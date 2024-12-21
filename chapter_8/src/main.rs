use std::{sync::atomic::{AtomicU32, Ordering}, thread, time::Duration};

#[cfg(not(target_os = "linux"))]
compile_error!("Linux only, sorry")

fn main() {
    println!("Hello, world!");

    let a = AtomicU32::new(0);

    thread::scope(|s| {
        s.spawn(|| {
            thread::sleep(Duration::from_secs(3));
            a.store(1, Ordering::Relaxed);
            awake_one(&a);
        });

        println!("Waiting ...");
        while a.load(Ordering::Relaxed) != 1 {
            wait(&a, 0);
        }
        println!("Finished")
    });
}

pub fn wait(a: &AtomicU32, expected: u32) {
    unsafe {
        libc::syscall(
            libc::SYS_futex,
            a as *const AtomicU32,
            libc::FUTEX_WAIT,
            expected,
            std::ptr::null::<libc::timespec>(),
        );
    }
}

pub fn awake_one(a: &AtomicU32) {
    unsafe {
        libc::syscall(
            libc::SYS_futex, 
            a as *const AtomicU32, 
            libc::FUTEX_WAKE, 
            1
        );
    }
}
