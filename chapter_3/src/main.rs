use std::sync::atomic::fence;
use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicPtr;
use std::thread;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::Ordering;
use std::time::Duration;

fn main() {
    println!("Hello, world!");
    happens_before();


    release_aquire_unsafe();

    how_to_mutex();

    println!("{:?}", get_data());
    seq_ordering();

    fences();
}

fn fences() {
    static mut DATA: [usize; 10] = [0; 10];

    const ATOMIC_FALSE: AtomicBool = AtomicBool::new(false);
    static READY: [AtomicBool; 10] = [ATOMIC_FALSE; 10];
    
    fn calculate_data(val: usize) -> usize {
        thread::sleep(Duration::from_millis(500));
        val
    }

    for i in  0..10 {
        thread::spawn(move || {
            let data = calculate_data(i);
            unsafe { DATA[i] = data };
            READY[i].store(true, Ordering::Release);
        });
    }

    thread::sleep(Duration::from_millis(500));
    let ready: [bool; 10] = std::array::from_fn(|i| READY[i].load(Ordering::Relaxed));
    
    if ready.contains(&true) {
        fence(Ordering::Acquire);
        
        for i in 0..10 {
            if ready[i] {
                println!("data[i] = {}", unsafe { DATA[i] });
            }
        }
    }

}


static mut DATA: String = String::new();
static LOCKED: AtomicBool = AtomicBool::new(false);

fn seq_ordering() {
    static A: AtomicBool = AtomicBool::new(false);
    static B: AtomicBool = AtomicBool::new(false);
    static mut S:String = String::new();

    let t1 = thread::spawn(|| {
        A.store(true, Ordering::SeqCst);
        if !B.load(Ordering::SeqCst) {
            unsafe { S.push_str("!A") };
        }
    });

    let t2 = thread::spawn(|| {
        B.store(true, Ordering::SeqCst);
        if !A.load(Ordering::SeqCst) {
            unsafe { S.push_str("!B") };
        }
    });

    t1.join().unwrap();
    t2.join().unwrap();

    unsafe { println!("{}", S) };
}

fn get_data() -> &'static str {
    static PTR: AtomicPtr<String> = AtomicPtr::new(std::ptr::null_mut());

    let mut p = PTR.load(Ordering::Acquire);

    if p.is_null() {
        p = Box::into_raw(Box::new(generate_data()));

        if let Err(e) = PTR.compare_exchange(std::ptr::null_mut(), p, Ordering::Release, Ordering::Acquire) {
            drop(unsafe { Box::from_raw(p) });
            p = e;
        }
    }

    unsafe { &*p }
}

fn generate_data() -> String {
    String::from("asddgty")
}


fn how_to_mutex() {
    thread::scope(|s| {
        for _ in 0..100 {
            s.spawn(f1);
        }
    });
    unsafe { println!("{:?}", DATA) };
}

fn f1() {
    if LOCKED.compare_exchange(false, true, Ordering::Acquire, Ordering::Relaxed).is_ok() {
        unsafe { DATA.push('!') };
        LOCKED.store(false, Ordering::Release);
    }
}


fn release_aquire_unsafe() {
    static mut DATA: u64 = 0;
    static READY: AtomicBool = AtomicBool::new(false);

    thread::spawn(|| {
        unsafe { DATA = 123 };
        READY.store(true, Ordering::Relaxed);
    });

    while !READY.load(Ordering::Relaxed) {
        //thread::sleep(Duration::from_secs(1));
        println!("Waiting..");
    }
    println!("{}", unsafe { DATA });
}

fn release_aquire() {
    static DATA: AtomicU64 = AtomicU64::new(0);
    static READY: AtomicBool = AtomicBool::new(false);

    thread::spawn(|| {
        DATA.store(123, Ordering::Relaxed);
        READY.store(true, Ordering::Release);
    });

    while !READY.load(Ordering::Acquire) {
        thread::sleep(Duration::from_secs(1));
        println!("Waiting..");
    }
    println!("{}", DATA.load(Ordering::Relaxed));
}


static X: AtomicI32 = AtomicI32::new(0);

fn happens_before() {
    X.store(1, Ordering::Relaxed);
    let t = thread::spawn(f);
    X.store(2, Ordering::Relaxed);
    t.join().unwrap();
    X.store(3, Ordering::Relaxed);
}

fn f() {
    let x = X.load(Ordering::Relaxed);

    assert!(x == 1 || x == 2);
}
