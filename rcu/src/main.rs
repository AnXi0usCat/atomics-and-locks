use std::{
    marker::PhantomData,
    ops::Deref,
    ptr,
    sync::{
        atomic::{AtomicPtr, Ordering},
        Mutex,
    },
};

fn main() {
    println!("Hello, world!");
}

pub struct HazardRecord {
    pub hazard: AtomicPtr<()>,
}

// SAFETY: We guarantee that HazardRecordPtr pointers are safely shared across threads.
// This safety guarantee is your responsibility as the programmer.
unsafe impl Send for HazardRecord {}
unsafe impl Sync for HazardRecord {}

#[derive(Clone, Copy)]
struct Ptr(*mut ());

// SAFETY: We guarantee that Ptr pointers are safely shared across threads.
// This safety guarantee is your responsibility as the programmer.
unsafe impl Send for Ptr {}
unsafe impl Sync for Ptr {}

static UNINITIALIZED_FLAG: u8 = 0;

thread_local! {
    static HAZARD_RECORD: &'static HazardRecord = {
        let record = Box::new(HazardRecord {
            hazard: AtomicPtr::new(UNINITIALIZED_FLAG as *mut ()),
        });
        let record_ref = Box::leak(record);
        record_ref
    }
}

struct Rcu<T> {
    ptr: AtomicPtr<T>,
    registry: Mutex<Vec<&'static HazardRecord>>,
    retired_list: Mutex<Vec<Ptr>>,
}

impl<T> Rcu<T> {
    pub fn new(value: T) -> Self {
        Rcu {
            ptr: AtomicPtr::new(Box::into_raw(Box::new(value))),
            registry: Mutex::new(Vec::new()),
            retired_list: Mutex::new(Vec::new()),
        }
    }

    pub fn read(&self) -> ReadGuard<'_, T> {
        loop {
            let ptr = self.ptr.load(Ordering::Acquire);
            if ptr.is_null() {
                panic!("Failed to read pointer, poitner cannot be null");
            }
            if HAZARD_RECORD
                .try_with(|record| record.hazard.load(Ordering::Acquire))
                .unwrap()
                == UNINITIALIZED_FLAG as *mut ()
            {
                // set the hazard pointer to global registry
                self.set_hazard(ptr);
                HAZARD_RECORD
                    .with(|hazard| self.registry.lock().expect("Lock poisoned").push(&hazard));
            } else {
                self.set_hazard(ptr);
            }
            // check again to make sure it didnt change
            if self.ptr.load(Ordering::Acquire) == ptr {
                return ReadGuard {
                    ptr,
                    rcu: self,
                    _marker: PhantomData,
                };
            }
        }
    }

    pub fn write(&self, value: T) {
        let new_ptr = Box::into_raw(Box::new(value));
        let old_ptr = self.ptr.swap(new_ptr, Ordering::AcqRel);
        // retire the old pointer to the global retired list
        self.retire(old_ptr as *mut ());
    }

    // sets a hazard pointer to the current thread local storage
    fn set_hazard(&self, ptr: *mut T) {
        HAZARD_RECORD.with(|record| {
            record.hazard.store(ptr as *mut (), Ordering::Release);
        });
    }

    // clears a hazard pointer from the current thread local storage
    fn clear_hazard(&self) {
        HAZARD_RECORD.with(|record| {
            record.hazard.store(ptr::null_mut(), Ordering::Release);
        });
    }

    // get a snapshot of all published hazard pointers accros threads
    fn get_hazard_pointers(&self) -> Vec<*mut ()> {
        let registry = self.registry.lock().expect("Lock poisoned");
        registry
            .iter()
            .map(|record_ptr| record_ptr.hazard.load(Ordering::Acquire))
            .filter(|&ptr| !ptr.is_null())
            .collect()
    }

    // retires a pointer by adding it to the global retired list
    fn retire(&self, ptr: *mut ()) {
        let mut retired = self.retired_list.lock().expect("Lock poisoned");
        retired.push(Ptr(ptr));

        // set an arbitrary value for now, should be configurable
        if retired.len() >= 10 {
            self.scan_and_reclaim();
        }
    }

    // clears out pointers out of retired lisr periodically
    fn scan_and_reclaim(&self) {
        let hazards = self.get_hazard_pointers();
        let mut retired = self.retired_list.lock().expect("Lock poisoned");

        let mut i = 0;
        while i < retired.len() {
            let Ptr(ptr) = retired[i];
            if !hazards.iter().any(|&raw| raw == ptr) {
                let Ptr(old) = retired.swap_remove(i);
                // SAFETY: pointer was created from a boxed value
                unsafe { drop(Box::from_raw(old)) };
            } else {
                i += 1;
            }
        }
    }
}

impl<T> Drop for Rcu<T> {
    fn drop(&mut self) {
        // clear out the retired list so we will not leak memory
        self.scan_and_reclaim();
        // SAFETY: we are dropping the RCU, means ther are no longer any readers
        // so the value is safe to drop. Pointer was created from a box so we need
        // to box it again to drop it.
        drop(unsafe { Box::from_raw(self.ptr.load(Ordering::Acquire)) });
    }
}

struct ReadGuard<'a, T> {
    ptr: *const T,
    rcu: &'a Rcu<T>,
    _marker: PhantomData<&'a T>,
}

impl<T> Deref for ReadGuard<'_, T> {
    type Target = T;
    fn deref(&self) -> &Self::Target {
        // SAFETY: we know that if we obtained the read guard then the pointer is not null
        // and was inserted in to the global hazard registry, so it is safe to dereference
        unsafe { &*self.ptr }
    }
}

impl<T> Drop for ReadGuard<'_, T> {
    fn drop(&mut self) {
        // clear the hazard pointer
        self.rcu.clear_hazard();
    }
}

#[cfg(test)]
mod tests {

    use std::{
        rc,
        sync::Arc,
        thread::{self, sleep},
        time::Duration,
    };

    use super::*;

    #[test]
    fn test_rcu_basic() {
        let rcu = Rcu::new(10);

        // read the value
        {
            let guard = rcu.read();
            assert_eq!(10, *guard);
        }
        // update the value
        rcu.write(20);

        // read the updated value
        let guard = rcu.read();
        assert_eq!(20, *guard);
    }

    #[test]
    fn test_set_and_clear() {
        let rcu = Rcu::new(10);
        let record = rcu.read();

        let hazards = rcu.get_hazard_pointers();
        assert!(
            !hazards.is_empty(),
            "Hazard pointer registry unexpectedly empty!"
        );

        for &ptr in &hazards {
            let val = unsafe { *(ptr as *mut i32) };
            println!("Hazard pointer points to: {}", val);
            assert_eq!(val, 10, "Expected pointer value to be 10, but got {}", val);
        }
        // Clear hazard pointer
        drop(record);

        let hazards = rcu.get_hazard_pointers();
        for &ptr in &hazards {
            assert!(ptr.is_null(), "Pointer has not been cleared");
        }

        HAZARD_RECORD.with(|record| {
            assert!(
                record.hazard.load(Ordering::Acquire).is_null(),
                "Thread local hazard pointer is not null.",
            )
        });
    }

    #[test]
    fn test_retire_and_scan() {
        let rcu = Rcu::new(10);
        // read the value to create a hazard pointer
        let _ = rcu.read();
        // write new value to force the old one in the retired list
        rcu.write(20);

        // pointer should be in the retired list now
        assert!(
            !rcu.retired_list.lock().expect("lock poisoned").is_empty(),
            "Retired list is unexpectedly empty"
        );

        // if no pointer is in hazard list then it should be reclaimed
        rcu.scan_and_reclaim();

        assert!(
            rcu.retired_list.lock().expect("lock poisoned").is_empty(),
            "Pointer has not been reclaimed from the hazard list"
        );
    }

    #[test]
    fn test_one_hazard_per_thread() {
        let rcu = Rcu::new(10);
        // reading the value will push a pointer to a hazard list
        let value_1 = rcu.read();

        assert!(
            rcu.registry.lock().expect("lock poinsoned").len() == 1,
            "Should contain only one record per thread"
        );

        // second read should update exisitng hazard record
        let value_2 = rcu.read();

        assert!(
            rcu.registry.lock().expect("lock poinsoned").len() == 1,
            "Should contain only one record per thread"
        );

        drop(value_1);
        drop(value_2);
    }

    #[test]
    fn test_read_from_different_thread() {
        let rcu = Arc::new(Rcu::new(10));
        thread::scope(|scope| {
            let rcu_2 = rcu.clone();
            scope.spawn(move || {
                // read the value, should be equal to 10
                let value = rcu_2.read();
                assert_eq!(*value, 10, "Value should be equal to 10");

                // drop the value so no one is using it anymore
                drop(value);
                thread::sleep(Duration::from_millis(200));

                // read the value again, should be equal to 20
                let value = rcu_2.read();
                assert_eq!(*value, 20, "Value should be equal to 20");
            });
            thread::sleep(Duration::from_millis(100));
            rcu.write(20);
        });
        assert!(
            rcu.retired_list.lock().expect("lock poisoned").len() == 1,
            "Doesnt have exatrly 1 element in retired list"
        )
    }
}
