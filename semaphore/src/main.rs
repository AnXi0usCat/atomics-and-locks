use std::{cell::UnsafeCell, ops::Deref, sync::atomic::{AtomicU32, Ordering}, thread, time::Duration};

use atomic_wait::{wait, wake_one};

fn main() {
    println!("Hello, world!");
    let sem = Semaphore::new(32, 3);

    thread::scope(|s| {
        for _ in 0..10 {
            s.spawn(|| {
                let guard = sem.acquire();
                println!("thread: {:?} got {:?}", std::thread::current().id(), *guard);
                thread::sleep(Duration::from_secs(3));
            });
        }
    });
}

struct Semaphore<T> {
    counter: AtomicU32,
    data: UnsafeCell<T>
}

unsafe impl<T> Sync for Semaphore<T> where T: Send {}

impl<T> Semaphore<T> {

    pub fn new(value: T, num_threads: u32) -> Self {
        Semaphore {
            counter: AtomicU32::new(num_threads),
            data: UnsafeCell::new(value),
        }
    }

    pub fn acquire(&self) -> SemaphoreGuard<T> {
        let mut s = self.counter.load(Ordering::Acquire);
        loop {
            if s > 0 {
                match self.counter.compare_exchange(s, s - 1, Ordering::Acquire, Ordering::Relaxed) {
                    Ok(_) => return SemaphoreGuard { lock: self },
                    Err(e) => { s = e; continue; },
                }
            }

            if s < 1 {
                wait(&self.counter, s);
                s = self.counter.load(Ordering::Acquire);
            }
        }
    }
}

struct SemaphoreGuard<'a, T> {
    lock: &'a Semaphore<T>
}

impl<T> Deref for SemaphoreGuard<'_, T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        unsafe { & *self.lock.data.get() }
    }
}

impl<T> Drop for SemaphoreGuard<'_,T> {
    fn drop(&mut self) {
        self.lock.counter.fetch_add(1, Ordering::Release);
        wake_one(&self.lock.counter);
    }
}
