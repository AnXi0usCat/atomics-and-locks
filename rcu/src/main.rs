use std::{
    marker::PhantomData,
    ops::Deref,
    ptr,
    sync::{
        atomic::{AtomicPtr, Ordering}, Mutex
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

thread_local! {
    static HAZARD_RECORD: HazardRecord = HazardRecord {
            hazard: AtomicPtr::new(ptr::null_mut()),
    }
}

struct Rcu<T> {
    ptr: AtomicPtr<T>,
    registry: Mutex<Vec<HazardRecord>>,
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
            // set the hazard pointer to global registry
            self.set_hazard(ptr);

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

        println!("--- Hazard pointers snapshot ---");
        let hazards = rcu.get_hazard_pointers();

        assert!(
            !hazards.is_empty(),
            "Hazard pointer registry unexpectedly empty!"
        );

        for &ptr in &hazards {
            let val = unsafe { *(ptr as *mut i32) };
            println!("Hazard pointer points to: {}", val);
            assert_eq!(val, 42, "Expected pointer value to be 42, but got {}", val);
        }

        // Clear hazard pointer
        rcu.clear_hazard();
    }

    #[test]
    fn test_retire_and_scan() {
        let rcu = Rcu::new(10);
        let hazards = rcu.get_hazard_pointers();
        
        let raw = *hazards.get(0).unwrap();
        // add pointer to the retire list
        rcu.retire(raw);

        // pointer should be in the retired list now
        assert!(rcu.retired_list
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|&Ptr(ptr)| ptr == raw));

        // if no pointer is in hazard list then it should be reclaimed
        rcu.scan_and_reclaim();

        assert!(!rcu.retired_list
            .lock()
            .expect("lock poisoned")
            .iter()
            .any(|&Ptr(ptr)| ptr == raw));
    }
}
