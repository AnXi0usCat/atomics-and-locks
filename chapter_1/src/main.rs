use std::sync::Condvar;
use std::thread;
use std::{
    cell::Cell,
    collections::VecDeque,
    sync::{Arc, Mutex},
    time::Duration,
};

fn main() {
    println!("Hello, world!");
    let t1 = thread::spawn(f);
    let t2 = thread::spawn(f);

    println!("Hello from a main thread");
    t1.join().unwrap();
    t2.join().unwrap();

    pass_closure();
    return_from_thread();

    // scoped threads
    borrow_in_scope();

    // statics
    statics();

    // leaking memory
    let x: &'static [i32; 3] = Box::leak(Box::new([1, 2, 3]));
    thread::spawn(move || dbg!(x)).join().unwrap();
    thread::spawn(move || dbg!(x)).join().unwrap();

    // reference counting
    let arc = Arc::new([1, 2, 3]);
    let arc2 = arc.clone();
    thread::spawn(move || dbg!(arc)).join().unwrap();
    thread::spawn(move || dbg!(arc2)).join().unwrap();

    // naming clones - use shadowing
    let a = Arc::new([1, 2, 3]);
    thread::spawn({
        let a = a.clone();
        move || {
            dbg!(a);
        }
    })
    .join()
    .unwrap();
    dbg!(a);

    // Cell
    let cell = Cell::new(1);
    cell_func(&cell, &cell);

    let cell = Cell::new(vec![1]);

    cell_noncopy(&cell);
    dbg!("{:?}", cell.take());

    // mutex
    mutex();

    // parking threads
    //parking();
    
    println!("conditionals");
    // cond vars;
    conditional_vars();
}

// parking threads
fn parking() {
    let queue = Mutex::new(VecDeque::new());

    thread::scope(|s| {
        // consuming thread
        let t = s.spawn(|| loop {
            let item = queue.lock().unwrap().pop_front();
            if let Some(item) = item {
                dbg!(item);
            } else {
                thread::park();
            }
        });

        // producing thread
        for i in 0..3 {
            queue.lock().unwrap().push_back(i);
            t.thread().unpark();
            thread::sleep(Duration::from_secs(1));
        }
    });
}

// condvars
fn conditional_vars() {
    let queue = Mutex::new(VecDeque::new());
    let cond = Condvar::new();

    thread::scope(|s| {
        // consuming thread
        s.spawn(|| loop {
            let mut guard = queue.lock().unwrap();
            let item = loop {
                if let Some(item) = guard.pop_front() {
                    break item;
                } else {
                    guard = cond.wait(guard).unwrap();
                }
            };
            drop(guard);
            dbg!(item);
        });

        // producing thread
        for i in 0..3 {
            queue.lock().unwrap().push_back(i);
            cond.notify_one();
            thread::sleep(Duration::from_secs(1));
        }
    });
}

fn mutex() {
    let n = Mutex::new(0);

    thread::scope(|s| {
        for _ in 0..10 {
            s.spawn(|| {
                let mut guard = n.lock().unwrap();
                for _ in 0..1000 {
                    *guard += 1;
                }
            });
        }
    });
    println!("value of the mutex is {:?}", n.lock().unwrap());
}

// interior mutability
// cell
fn cell_func(a: &Cell<i32>, b: &Cell<i32>) {
    let before = a.get();
    b.set(b.get() + 1);
    let after = a.get();
    if before != after {
        println!("The were the same!!");
    }
}

// cell with non copy types
fn cell_noncopy(a: &Cell<Vec<i32>>) {
    let mut copy = a.take();
    copy.push(2);
    a.set(copy);
}

// statics
static X: [i32; 3] = [1, 2, 3];

fn statics() {
    thread::spawn(|| dbg!(&X)).join().unwrap();
    thread::spawn(|| dbg!(&X)).join().unwrap();
}

fn f() {
    println!("Hello from another thread");

    let id = thread::current().id();
    println!("This is my thread id {id:?}");
}

fn pass_closure() {
    let v = vec![1, 2, 3];

    thread::spawn(move || {
        for el in v {
            println!("{el}");
        }
    })
    .join()
    .unwrap();
}

fn return_from_thread() {
    let numbers = Vec::from_iter(0..=1000);
    let average = thread::spawn(move || {
        let len = numbers.len();
        let sum = numbers.into_iter().sum::<usize>();
        sum / len
    })
    .join()
    .unwrap();

    println!("the average is {average}");
}

fn borrow_in_scope() {
    let numbers = vec![1, 2, 3];
    thread::scope(|s| {
        s.spawn(|| {
            let sum = numbers.iter().sum::<usize>();
            println!("sum of nums is {sum:?}");
        });
        s.spawn(|| {
            for i in &numbers {
                println!("{}", *i);
            }
        });
    });
}
