use std::{
    ops::Deref,
    sync::atomic::{AtomicPtr, AtomicU32, Ordering},
    thread,
    time::Duration,
};

use atomic_wait::{wait, wake_one};

fn main() {
    println!("Hello, world!");
    let rcu = Rcu::new(10);

    thread::scope(|s| {
        s.spawn(|| {
            println!("{}", *rcu.read());
            thread::sleep(Duration::from_secs(1));
            println!("{}", *rcu.read());
        });
        thread::sleep(Duration::from_millis(1));
        rcu.write(12);
    });
}

struct Rcu<T> {
    pointer: AtomicPtr<T>,
    readers: AtomicU32,
}

unsafe impl<T> Sync for Rcu<T> where T: Send + Sync {}

impl<T> Rcu<T> {
    pub fn new(value: T) -> Self {
        Rcu {
            pointer: AtomicPtr::new(Box::into_raw(Box::new(value))),
            readers: AtomicU32::new(0),
        }
    }

    pub fn read(&self) -> ReadGuard<'_, T> {
        self.readers.fetch_add(1, Ordering::Release);
        ReadGuard { rcu: self }
    }

    pub fn write(&self, value: T) {
        let new_ptr = Box::into_raw(Box::new(value));
        let mut r = self.readers.load(Ordering::Acquire);
        let mut old_ptr = self.pointer.load(Ordering::Acquire);
        loop {
            if r < 1 {
                match self.pointer.compare_exchange(
                    old_ptr,
                    new_ptr,
                    Ordering::Acquire,
                    Ordering::Relaxed,
                ) {
                    Ok(_) => {
                        drop(unsafe { Box::from_raw(old_ptr) });
                        return;
                    }
                    Err(e) => {
                        old_ptr = e;
                        continue;
                    }
                }
            }
            if r > 0 {
                wait(&self.readers, r);
                r = self.readers.load(Ordering::Acquire);
                old_ptr = self.pointer.load(Ordering::Acquire);
                continue;
            }
        }
    }
}

impl<T> Drop for Rcu<T> {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.pointer.load(Ordering::Acquire)) });
    }
}

struct ReadGuard<'a, T> {
    rcu: &'a Rcu<T>,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe {
            &*self
                .rcu
                .pointer
                .load(Ordering::Acquire)
                .as_ref()
                .expect("Failed to obtain a a shared reference")
        }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        if self.rcu.readers.fetch_sub(1, Ordering::Release) == 1 {
            wake_one(&self.rcu.readers);
        }
    }
}
