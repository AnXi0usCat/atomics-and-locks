use std::sync::atomic::{AtomicI32, Ordering};

#[inline(never)]
pub fn fetch_add(x: &AtomicI32) {
    x.fetch_add(10, Ordering::Relaxed);
}

#[inline(never)]
pub fn fetch_or(x: &AtomicI32) -> i32 {
    x.fetch_or(10, Ordering::Relaxed)
}

#[inline(never)]
pub fn fetc_or_manual(x: &AtomicI32) -> i32 {
    let mut current = x.load(Ordering::Relaxed);
    loop {
        let new = current | 10;
        match x.compare_exchange(current, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(v) => return v,
            Err(e) => current = e,
        }
    }
}

#[inline(never)]
pub fn add_ten(num: &mut i32) {
    *num += 10;
}

#[inline(never)]
pub fn atomic_add_ten(num: &AtomicI32) -> i32 {
    num.fetch_add(10, Ordering::Relaxed)
}

#[inline(never)]
pub fn store(num: &mut i32) {
    *num = 0;
}


#[inline(never)]
pub fn atomic_store(num: &AtomicI32) {
    num.store(0, Ordering::Relaxed);
}

#[inline(never)]
pub fn load(num: &mut i32) -> i32 {
    *num
}


#[inline(never)]
pub fn atomic_load(num: &AtomicI32) -> i32 {
    num.load(Ordering::Relaxed)
}
