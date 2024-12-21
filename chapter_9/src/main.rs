use std::{
    cell::UnsafeCell,
    ops::{Deref, DerefMut},
    sync::atomic::{AtomicU32, AtomicUsize, Ordering},
    u32,
};

use atomic_wait::{wait, wake_all, wake_one};

fn main() {
    println!("Hello, world!");
}

struct Mutex<T> {
    // 0 is uncloked
    // 1 is locked, no threads waiting
    // 2 is locked, threads are waiting
    state: AtomicU32,
    data: UnsafeCell<T>,
}

unsafe impl<T> Sync for Mutex<T> where T: Send {}

impl<T> Mutex<T> {
    #[inline]
    pub fn new(data: T) -> Self {
        Mutex {
            state: AtomicU32::new(0),
            data: UnsafeCell::new(data),
        }
    }

    #[inline]
    pub fn lock(&self) -> MutexGuard<T> {
        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            self.lock_contnded();
        }

        MutexGuard { lock: self }
    }

    #[cold]
    #[inline]
    fn lock_contnded(&self) {
        let mut counter = 0;

        while self.state.load(Ordering::Relaxed) == 1 && counter < 100 {
            counter += 1;
            std::hint::spin_loop();
        }

        if self
            .state
            .compare_exchange(0, 1, Ordering::Acquire, Ordering::Relaxed)
            .is_ok()
        {
            return;
        }

        while self.state.swap(2, Ordering::Acquire) != 0 {
            wait(&self.state, 2);
        }
    }
}

struct MutexGuard<'a, T> {
    lock: &'a Mutex<T>,
}

impl<T> Deref for MutexGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for MutexGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for MutexGuard<'_, T> {
    #[inline]
    fn drop(&mut self) {
        if self.lock.state.swap(0, Ordering::Release) == 2 {
            // wake up one thread
            wake_one(&self.lock.state)
        }
    }
}

struct RwLock<T> {
    // number of readers * 2 + 1 if there is a writer waiting when reader lock
    // is aquired or u33::MAX if writer lock is aquired
    state: AtomicU32,
    write_lock_counter: AtomicU32,
    data: UnsafeCell<T>,
}

impl<T> RwLock<T> {
    pub fn new(value: T) -> Self {
        RwLock {
            state: AtomicU32::new(0),
            write_lock_counter: AtomicU32::new(0),
            data: UnsafeCell::new(value),
        }
    }

    pub fn read(&self) -> ReadGuard<T> {
        let mut s = self.state.load(Ordering::Relaxed);
        loop {
            if s % 2 == 0 {
                assert!(s != u32::MAX - 2, "too many readers");
                match self.state.compare_exchange_weak(
                    s,
                    s + 2,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => return ReadGuard { lock: self },
                    Err(e) => s = e,
                }
            }

            if s == u32::MAX {
                wait(&self.state, s);
                s = self.state.load(Ordering::Relaxed);
            }
        }
    }

    pub fn write(&self) -> WriteGuard<T> {
        let mut s = self.state.load(Ordering::Relaxed);
        loop {
            // see if we can aquire the lock
            if s <= 1 {
                match self
                    .state
                    .compare_exchange(s, u32::MAX, Ordering::Acquire, Ordering::Relaxed)
                {
                    Ok(_) => return WriteGuard { lock: self },
                    Err(e) => {
                        s = e;
                        continue;
                    }
                }
            }
            // try to block new readers if no writers are waiting
            if s % 2 == 0 {
                match self
                    .state
                    .compare_exchange(s, s + 1, Ordering::Relaxed, Ordering::Relaxed)
                {
                    Ok(_) => {}
                    Err(e) => {
                        s = e;
                        continue;
                    }
                }
            }
            let w = self.write_lock_counter.fetch_add(1, Ordering::Acquire);
            s = self.state.load(Ordering::Relaxed);
            // if readers and maybe writer waiting, go to sleep
            if s >= 2 {
                wait(&self.write_lock_counter, w);
                s = self.state.load(Ordering::Relaxed);
            }
        }
    }
}

unsafe impl<T> Sync for RwLock<T> where T: Send + Sync {}

struct ReadGuard<'a, T> {
    lock: &'a RwLock<T>,
}

struct WriteGuard<'a, T> {
    lock: &'a RwLock<T>,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> Deref for WriteGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.lock.data.get() }
    }
}

impl<T> DerefMut for WriteGuard<'_, T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { &mut *self.lock.data.get() }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        if self.lock.state.fetch_sub(2, Ordering::Release) == 3 {
            self.lock.write_lock_counter.fetch_add(1, Ordering::Release);
            wake_one(&self.lock.write_lock_counter);
        }
    }
}

impl<T> Drop for WriteGuard<'_, T> {
    fn drop(&mut self) {
        self.lock.state.store(0, Ordering::Release);
        self.lock.write_lock_counter.fetch_add(1, Ordering::Release);
        wake_one(&self.lock.write_lock_counter);
        wake_all(&self.lock.state);
    }
}

struct CondVar {
    counter: AtomicU32,
    waiters: AtomicUsize,
}

impl CondVar {
    pub fn new() -> Self {
        CondVar {
            counter: AtomicU32::new(0),
            waiters: AtomicUsize::new(0),
        }
    }

    pub fn notify_one(&self) {
        if self.waiters.load(Ordering::Relaxed) > 0 {
            self.counter.fetch_add(1, Ordering::Relaxed);
            wake_one(&self.counter);
        }
    }

    pub fn notify_all(&self) {
        if self.waiters.load(Ordering::Relaxed) > 0 {
            self.counter.fetch_add(1, Ordering::Relaxed);
            wake_all(&self.counter);
        }
    }

    pub fn wait<'a, T>(&self, guard: MutexGuard<'a, T>) -> MutexGuard<'a, T> {
        self.waiters.fetch_add(1, Ordering::Relaxed);

        let count = self.counter.load(Ordering::Relaxed);

        let mutex = guard.lock;
        drop(guard);

        wait(&self.counter, count);

        self.waiters.fetch_sub(1, Ordering::Relaxed);
        mutex.lock()
    }
}

#[cfg(test)]
mod tests {
    use super::{CondVar, Mutex};
    use std::{thread, time::Duration};

    #[test]
    fn cond_vars() {
        let mutex = Mutex::new(0);
        let cond = CondVar::new();

        let mut wakeups = 0;

        thread::scope(|s| {
            s.spawn(|| {
                thread::sleep(Duration::from_secs(2));
                *mutex.lock() = 123;
                cond.notify_one();
            });

            let mut m = mutex.lock();
            while *m < 100 {
                m = cond.wait(m);
                wakeups += 1;
            }
        });

        assert!(wakeups < 10);
    }
}
