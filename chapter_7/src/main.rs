use std::{hint::black_box, sync::atomic::{AtomicU64, Ordering}, thread, time::SystemTime};

#[repr(align(64))]
struct Aligned(AtomicU64);

fn main() {
    black_box(&A);
    let start_time = SystemTime::now();
    thread::spawn(|| {
        loop {
            A[0].0.store(1, Ordering::Relaxed);
            A[2].0.store(2, Ordering::Relaxed);
        }
    });

    for _ in 0..1_000_000_000 {
        black_box(A[1].0.load(Ordering::Relaxed));
    }

    println!("elapsed: {:?}", start_time.elapsed());

}


static A: [Aligned; 3] = [
    Aligned(AtomicU64::new(0)),
    Aligned(AtomicU64::new(0)),
    Aligned(AtomicU64::new(0)),
]; 
