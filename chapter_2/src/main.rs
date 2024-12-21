use std::sync::atomic::AtomicBool;
use std::sync::atomic::AtomicI32;
use std::sync::atomic::AtomicU64;
use std::sync::atomic::AtomicUsize;
use std::sync::atomic::Ordering;
use std::sync::Once;
use std::thread;
use std::time::Duration;
use std::time::Instant;

fn main() {
    println!("Hello, world!");

    // stop_flag();

    // report_progress();

    (0..3).map(|_| thread::spawn(|| get_x())).for_each(|i| {
        i.join().unwrap();
    });

    println!("multi-threaded report progress");
    report_progress_multi();
}

// compareand exchange operations

fn increment(a: &AtomicI32) {
    let mut cur = a.load(Ordering::Relaxed);

    loop {
        let new = cur + 1;
        match a.compare_exchange(cur, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => return,
            Err(v) => cur = v
        }
    }
}

fn allocate_new_id() -> i32 {
    static NEXT_ID: AtomicI32 = AtomicI32::new(0);
    let mut cur = NEXT_ID.load(Ordering::Relaxed);

    loop {
        assert!(cur < 1000, "too many ID's");
        match NEXT_ID.compare_exchange(cur, cur + 1, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(v) => return v,
            Err(v) => cur = v,
        }
    }
}

// lazy one time init
fn get_key() -> u64 {
    static KEY1: AtomicU64 = AtomicU64::new(0);
    let key = KEY1.load(Ordering::Relaxed);

    if key == 0 {
        let new = generate_random_key();
        match KEY1.compare_exchange(key, new, Ordering::Relaxed, Ordering::Relaxed) {
            Ok(_) => new,
            Err(v) => v,
        }
    } else {
        key
    }
}

fn generate_random_key() -> u64 {
    1234567876543 as u64
}

fn get_x() -> usize {
    static ONCE: Once = Once::new();
    static X: AtomicUsize = AtomicUsize::new(0);

    ONCE.call_once(|| {
        println!("Setting X");
        let x = calculate_x();
        X.store(x, Ordering::Relaxed);
    });

    println!("loading X");
    X.load(Ordering::Relaxed)
}

fn calculate_x() -> usize {
    thread::sleep(Duration::from_secs(5));
    123456789
}

fn stop_flag() {
    static STOP: AtomicBool = AtomicBool::new(false);

    let background_thread = thread::spawn(|| {
        while !STOP.load(Ordering::Relaxed) {
            do_work();
        }
    });

    for line in std::io::stdin().lines() {
        match line.unwrap().as_str() {
            "help" => println!("commands: help, stop"),
            "stop" => break,
            cmd => println!("unknown command: {cmd:?}"),
        }
    }

    STOP.store(true, Ordering::Relaxed);

    background_thread.join().unwrap();
}

fn do_work() {
    thread::sleep(Duration::from_millis(200));
}

fn report_progress() {
    let num_done = AtomicUsize::new(0);
    let main_thread = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            for i in 0..100 {
                do_work();
                num_done.store(i + 1, Ordering::Relaxed);
                main_thread.unpark();
            }
        });

        loop {
            let n = num_done.load(Ordering::Relaxed);
            if n == 100 {
                break;
            }
            println!("Comlted {n:?}/100");
            thread::park_timeout(Duration::from_secs(1));
        }
    });
    println!("Done");
}

fn report_progress_multi() {
    let num_done = &AtomicUsize::new(0);
    let total_time = &AtomicU64::new(0);
    let max_time = &AtomicU64::new(0);
    thread::scope(|s| {
        for t in 0..4 {
            s.spawn(move || {
                for i in 0..25 {
                    let start = Instant::now();
                    println!("{:?}: processing: {:?}", thread::current().id(), 25 * t + i);
                    do_work();
                    let time_taken = start.elapsed().as_micros() as u64;
                    total_time.fetch_add(time_taken, Ordering::Relaxed);
                    max_time.fetch_max(time_taken, Ordering::Relaxed);
                    num_done.fetch_add(1, Ordering::Relaxed);
                }
            });
        }

        loop {
            let max_time = Duration::from_micros(max_time.load(Ordering::Relaxed));
            let total_time = Duration::from_micros(total_time.load(Ordering::Relaxed));
            let n = num_done.load(Ordering::Relaxed);
            if n == 100 {
                break;
            }

            if n == 0 {
                println!("Starting work, nothing is done");
            } else {
                println!(
                    "Working.. {n}/100 done, {:?} average, {:?} peak",
                    total_time / n as u32,
                    max_time
                );
            }
            thread::sleep(Duration::from_secs(1));
        }
    });
    println!("Done");
}
