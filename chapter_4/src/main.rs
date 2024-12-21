use std::ops::{Deref, DerefMut};
use std::thread;
use std::{
    cell::UnsafeCell,
    sync::atomic::{AtomicBool, Ordering},
};

fn main() {
    println!("Hello, spin lock!");

    let l = SpinLock::new(Vec::new());
    thread::scope(|s| {
        s.spawn(|| {
            l.lock().push(1);
        });
        s.spawn(|| {
            let mut guard = l.lock();
            guard.push(1);
            guard.push(2);
        });
    });

    let guard = l.lock();
    assert!(guard.as_slice() == [1,1,2] || guard.as_slice() == [1, 2, 1]);
}

struct SpinLock<T> {
    locked: AtomicBool,
    value: UnsafeCell<T>,
}

unsafe impl<T> Sync for SpinLock<T> where T: Send {}

impl<T> SpinLock<T> {
    pub const fn new(value: T) -> Self {
        Self {
            locked: AtomicBool::new(false),
            value: UnsafeCell::new(value),
        }
    }

    pub fn lock(&self) -> Guard<T> {
        while self.locked.swap(true, Ordering::Acquire) {
            std::hint::spin_loop();
        }
        Guard { lock: self }
    }
}

pub struct Guard<'a, T> {
    lock: &'a SpinLock<T>,
}

impl<T> Deref for Guard<'_, T> {
    type Target = T;
    fn deref(&self) -> &T {
        unsafe { &*self.lock.value.get() }
    }
}


impl<T> DerefMut for Guard<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
       unsafe {&mut *self.lock.value.get() } 
    }
}

impl<T> Drop for Guard<'_, T> {
    fn drop(&mut self) {
        self.lock.locked.store(false, Ordering::Release);
    }
}
