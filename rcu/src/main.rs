use std::{
    marker::PhantomData,
    ops::Deref,
    sync::atomic::{AtomicPtr, Ordering},
    thread,
    time::Duration,
};

fn main() {
    println!("Hello, world!");
    let rcu = Rcu::new(10);

    thread::scope(|s| {
        s.spawn(|| {
            let mut reader = rcu.read();
            println!("{}", *reader);
            drop(reader);
            thread::sleep(Duration::from_secs(1));
            reader = rcu.read();
            println!("{}", *reader);
        });
        thread::sleep(Duration::from_millis(1));
        rcu.write(12);
    });
}

struct Rcu<T> {
    ptr: AtomicPtr<T>,
}

impl<T> Rcu<T> {
    pub fn new(value: T) -> Self {
        Rcu {
            ptr: AtomicPtr::new(Box::into_raw(Box::new(value))),
        }
    }

    pub fn read(&self) -> ReadGuard<'_, T> {
        loop {
            let ptr = self.ptr.load(Ordering::Acquire);
            if ptr.is_null() {
                panic!("Failed to read pointer, poitner cannot be null");
            }
            // check again to make sure it didnt change
            if self.ptr.load(Ordering::Acquire) == ptr {
                return ReadGuard {
                    ptr,
                    _marker: PhantomData,
                };
            }
        }
    }

    pub fn write(&self, value: T) -> *mut T {
        let new_ptr = Box::into_raw(Box::new(value));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        old_ptr
    }
}

impl<T> Drop for Rcu<T> {
    fn drop(&mut self) {
        drop(unsafe { Box::from_raw(self.ptr.load(Ordering::Acquire)) });
    }
}

struct ReadGuard<'a, T> {
    ptr: *const T,
    _marker: PhantomData<&'a T>,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        unsafe { &*self.ptr }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        // will use this later to clear the hazard pointer
    }
}

#[cfg(test)]
mod tests {

    use super::*;

    #[test]
    fn simple_case() {
        let rcu = Rcu::new(10);

        thread::scope(|s| {
            s.spawn(|| {
                assert_eq!(10, *rcu.read());
                thread::sleep(Duration::from_secs(1));
                assert_eq!(12, *rcu.read());
            });
            thread::sleep(Duration::from_millis(1));
            rcu.write(12);
        });
    }

    #[test]
    fn test_rcu_basic() {
        let rcu = Rcu::new(10);

        // read the value
        {
            let guard = rcu.read();
            assert_eq!(10, *guard);
        }
        // update the value
        let old_ptr = rcu.write(20);
        unsafe {
            drop(Box::from_raw(old_ptr));
        }

        // read the updated value
        let guard = rcu.read();
        assert_eq!(20, *guard);
    }
}
