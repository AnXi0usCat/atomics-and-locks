use std::ops::Deref;
use std::{
    cell::UnsafeCell,
    mem::ManuallyDrop,
    ptr::NonNull,
    sync::atomic::{fence, AtomicUsize, Ordering},
    usize,
};

fn main() {
    println!("Hello, world!");
}

struct ArcData<T> {
    data: UnsafeCell<ManuallyDrop<T>>,
    data_count: AtomicUsize,
    alloc_count: AtomicUsize,
}

struct Arc<T> {
    pointer: NonNull<ArcData<T>>,
}

struct Weak<T> {
    pointer: NonNull<ArcData<T>>,
}

unsafe impl<T> Send for Arc<T> where T: Send + Sync {}
unsafe impl<T> Sync for Arc<T> where T: Sync + Sync {}

unsafe impl<T> Send for Weak<T> where T: Send + Sync {}
unsafe impl<T> Sync for Weak<T> where T: Sync + Sync {}

impl<T> Arc<T> {
    pub fn new(value: T) -> Self {
        let ptr = unsafe {
            NonNull::new_unchecked(Box::leak(Box::new(ArcData {
                data: UnsafeCell::new(ManuallyDrop::new(value)),
                data_count: AtomicUsize::new(1),
                alloc_count: AtomicUsize::new(1),
            })))
        };

        Arc { pointer: ptr }
    }

    fn data(&self) -> &ArcData<T> {
        unsafe { self.pointer.as_ref() }
    }

    pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
        if arc
            .data()
            .alloc_count
            .compare_exchange(1, usize::MAX, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            return None;
        }
        let is_unique = arc.data().data_count.load(Ordering::Relaxed) == 1;
        arc.data().alloc_count.store(1, Ordering::Release);

        if !is_unique {
            return None;
        }

        fence(Ordering::Acquire);
        unsafe { Some(&mut *arc.data().data.get()) }
    }

    pub fn downgrade(arc: &Self) -> Weak<T> {
        let mut n = arc.data().alloc_count.load(Ordering::Relaxed);
        loop {
            if n == usize::MAX {
                std::hint::spin_loop();
                n = arc.data().alloc_count.load(Ordering::Relaxed);
                continue;
            }
            assert!(n < usize::MAX - 1);
            if let Err(e) = arc.data().alloc_count.compare_exchange_weak(
                n,
                n + 1,
                Ordering::Acquire,
                Ordering::Relaxed,
            ) {
                n = e;
                continue;
            }
            return Weak {
                pointer: arc.pointer,
            };
        }
    }
}

impl<T> Weak<T> {
    fn data(&self) -> &ArcData<T> {
        unsafe { self.pointer.as_ref() }
    }

    pub fn upgrade(&self) -> Option<Arc<T>> {
        let mut n = self.data().data_count.load(Ordering::Relaxed);
        loop {
            if n == 0 {
                return None;
            }

            assert!(n < usize::MAX);

            if let Err(e) = self.data().data_count.compare_exchange_weak(
                n,
                n + 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                n = e;
                continue;
            }

            return Some(Arc {
                pointer: self.pointer,
            });
        }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &Self::Target {
        let ptr = self.data().data.get();
        unsafe { &(*ptr) }
    }
}

impl<T> Clone for Arc<T> {
    fn clone(&self) -> Self {
        if self.data().data_count.fetch_add(1, Ordering::Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }

        Arc {
            pointer: self.pointer,
        }
    }
}

impl<T> Clone for Weak<T> {
    fn clone(&self) -> Self {
        if self.data().alloc_count.fetch_add(1, Ordering::Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }

        Weak {
            pointer: self.pointer,
        }
    }
}

impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        if self.data().data_count.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);

            unsafe { ManuallyDrop::drop(&mut *self.data().data.get()) };
        }

        drop(Weak {
            pointer: self.pointer,
        });
    }
}

impl<T> Drop for Weak<T> {
    fn drop(&mut self) {
        if self.data().alloc_count.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);
            unsafe { drop(Box::from_raw(self.pointer.as_ptr())) };
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Arc;
    use std::{
        sync::atomic::{AtomicUsize, Ordering},
        thread,
    };

    #[test]
    fn test_arc() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug, PartialEq)]
        struct NumDrops;

        impl Drop for NumDrops {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }

        let x = Arc::new(("hello", NumDrops));
        let mut y = x.clone();

        let z = Arc::get_mut(&mut y);
        assert_eq!(z, None);

        let t = thread::spawn(move || {
            assert_eq!(x.0, "hello");
        });

        assert_eq!(y.0, "hello");

        t.join().unwrap();

        // one Arc should have been dropped by now
        assert_eq!(NUM_DROPS.load(Ordering::Relaxed), 0);

        let z = Arc::get_mut(&mut y);
        assert_eq!(z.unwrap().0, "hello");

        drop(y);

        assert_eq!(NUM_DROPS.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_weak() {
        static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);

        #[derive(Debug, PartialEq)]
        struct NumDrops;

        impl Drop for NumDrops {
            fn drop(&mut self) {
                NUM_DROPS.fetch_add(1, Ordering::Relaxed);
            }
        }

        let x = Arc::new(("hello", NumDrops));
        let y = Arc::downgrade(&x);
        let z = Arc::downgrade(&x);

        let t = thread::spawn(move || {
            // should be upgradable
            let arc = y.upgrade().unwrap();
            assert_eq!(arc.0, "hello");
        });

        assert_eq!(x.0, "hello");
        t.join().unwrap();

        // data shouldnt be dropped yet
        // and the weak should be upgradable
        assert_eq!(NUM_DROPS.load(Ordering::Relaxed), 0);
        assert!(z.upgrade().is_some());

        drop(x);

        // data should get dropped and the weak is no longer upgradable
        assert_eq!(NUM_DROPS.load(Ordering::Relaxed), 1);
        assert!(z.upgrade().is_none());
    }
}
