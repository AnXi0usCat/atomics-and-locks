use std::{
    cell::UnsafeCell, collections::VecDeque, marker::PhantomData, mem::MaybeUninit, sync::{
        atomic::{AtomicBool, AtomicU8, Ordering},
        Arc, Condvar, Mutex,
    }, thread::{self, Thread}
};

fn main() {
    println!("Hello, world!");
    test_one_shot_channel();
    test_compile_check_chanel();
    test_channel_ref();
}

struct Channel<T> {
    queue: Mutex<VecDeque<T>>,
    is_ready: Condvar,
}

impl<T> Channel<T> {
    fn new() -> Self {
        Channel {
            queue: Mutex::new(VecDeque::new()),
            is_ready: Condvar::new(),
        }
    }

    pub fn send(&self, value: T) {
        self.queue.lock().unwrap().push_back(value);
        self.is_ready.notify_one();
    }

    pub fn receive(&self) -> T {
        let mut b = self.queue.lock().unwrap();
        loop {
            if let Some(value) = b.pop_front() {
                return value;
            }
            b = self.is_ready.wait(b).unwrap();
        }
    }
}

struct OneShotChannel<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    is_ready: AtomicBool,
    in_use: AtomicBool,
}

unsafe impl<T> Sync for OneShotChannel<T> where T: Send {}

impl<T> OneShotChannel<T> {
    pub fn new() -> Self {
        OneShotChannel {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            is_ready: AtomicBool::new(false),
            in_use: AtomicBool::new(false),
        }
    }

    pub fn send(&self, value: T) {
        if self.in_use.swap(true, Ordering::Acquire) {
            panic!("Can't send more than one message");
        }
        unsafe { (*self.value.get()).write(value) };
        self.is_ready.store(true, Ordering::Release)
    }

    pub fn is_ready(&self) -> bool {
        self.is_ready.load(Ordering::Relaxed)
    }

    pub fn receive(&self) -> T {
        if !self.is_ready.swap(false, Ordering::Acquire) {
            panic!("Message is not rerady");
        }
        unsafe { (*self.value.get()).assume_init_read() }
    }
}

impl<T> Drop for OneShotChannel<T> {
    fn drop(&mut self) {
        if *self.is_ready.get_mut() {
            unsafe { self.value.get_mut().assume_init_drop() };
        }
    }
}

fn test_one_shot_channel() {
    let ch = StateOneShotChannel::new();
    let t = thread::current();
    thread::scope(|s| {
        s.spawn(|| {
            ch.send("hello world!");
            t.unpark();
        });
        while !ch.is_ready() {
            thread::park();
        }

        let val = ch.receive();
        assert_eq!(val, "hello world!");
    });
}

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READING: u8 = 2;
const READY: u8 = 2;

struct StateOneShotChannel<T> {
    value: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8,
}

unsafe impl<T: Send> Sync for StateOneShotChannel<T> {}

impl<T> StateOneShotChannel<T> {
    fn new() -> Self {
        StateOneShotChannel {
            value: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(EMPTY),
        }
    }

    pub fn send(&self, value: T) {
        if self
            .state
            .compare_exchange(EMPTY, WRITING, Ordering::Relaxed, Ordering::Relaxed)
            .is_err()
        {
            panic!("Can't send more than one message");
        }
        unsafe { (*self.value.get()).write(value) };
        self.state.store(READY, Ordering::Release);
    }

    pub fn is_ready(&self) -> bool {
        self.state.load(Ordering::Relaxed) == READY
    }

    pub fn receive(&self) -> T {
        if self
            .state
            .compare_exchange(READY, READING, Ordering::Acquire, Ordering::Relaxed)
            .is_err()
        {
            panic!("Message is not ready");
        }
        unsafe { (*self.value.get()).assume_init_read() }
    }
}

impl<T> Drop for StateOneShotChannel<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == READY {
            unsafe { (*self.value.get_mut()).assume_init_drop() };
        }
    }
}

pub struct Channel1<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    ready: AtomicBool,
}

unsafe impl<T: Send> Sync for Channel1<T> {}

pub struct Sender<T> {
    channel: Arc<Channel1<T>>,
}

struct Receiver<T> {
    channel: Arc<Channel1<T>>,
}

fn channel<T>() -> (Sender<T>, Receiver<T>) {
    let ch = Arc::new(Channel1 {
        message: UnsafeCell::new(MaybeUninit::uninit()),
        ready: AtomicBool::new(false),
    });

    (
        Sender {
            channel: ch.clone(),
        },
        Receiver { channel: ch },
    )
}

impl<T> Sender<T> {
    pub fn send(self, value: T) {
        unsafe { (*self.channel.message.get()).write(value) };
        self.channel.ready.store(true, Ordering::Release);
    }
}

impl<T> Receiver<T> {
    pub fn is_ready(&self) -> bool {
        self.channel.ready.load(Ordering::Relaxed)
    }

    pub fn receive(self) -> T {
        if !self.channel.ready.swap(false, Ordering::Acquire) {
            panic!("Message is not available");
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

impl<T> Drop for Channel1<T> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
            unsafe { self.message.get_mut().assume_init_drop() };
        }
    }
}

pub fn test_compile_check_chanel() {
    let (sender, receiver) = channel();
    let t = thread::current();

    thread::scope(|s| {
        s.spawn(|| {
            sender.send("hello cats");
            t.unpark();
        });

        while !receiver.is_ready() {
            thread::park();
        }
        assert!(receiver.receive() == "hello cats");
    });
}

pub struct Channel2<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    ready: AtomicBool,
}

impl<T> Channel2<T> {
    pub fn channel() -> Self {
        Channel2 {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            ready: AtomicBool::new(false),
        }
    }

    pub fn split<'a>(&'a mut self) -> (Sender2<'a, T>, Receiver2<'a, T>) {
        *self = Channel2::channel();
        (Sender2 { channel: self, thread: thread::current() }, Receiver2 { channel: self, _no_send: PhantomData } )
    }
}

unsafe impl<T: Send> Sync for Channel2<T> {}

pub struct Receiver2<'a, T> {
    channel: &'a Channel2<T>,
    _no_send: PhantomData<*const ()>
}

impl<T> Receiver2<'_, T> {
    pub fn is_ready(&self) -> bool {
        self.channel.ready.load(Ordering::Relaxed)
    }

    pub fn receive(&self) -> T {
        while !self.channel.ready.swap(false, Ordering::Acquire) {
            thread::park();
        }
        unsafe { (*self.channel.message.get()).assume_init_read() }
    }
}

pub struct Sender2<'a, T> {
    channel: &'a Channel2<T>,
    thread: Thread,
}

impl<T> Sender2<'_, T> {
    pub fn send(&self, message: T) {
        unsafe { (*self.channel.message.get()).write(message) };
        self.channel.ready.store(true, Ordering::Release);
        self.thread.unpark();
    }
}

impl<T> Drop for Channel2<T> {
    fn drop(&mut self) {
        if *self.ready.get_mut() {
           unsafe { (*self.message.get_mut()).assume_init_drop() };
        }
    }
}

fn test_channel_ref() {

    let mut channel = Channel2::channel();
    let (sender, receiver) = channel.split();

    thread::scope(|s| {

        s.spawn(move || {
            sender.send("hello");
        });

        assert!(receiver.receive() == "hello");
    });

}

struct Iter<'a, T: 'a> {
    ptr: *const T,
    end: *const T,
    _marker: PhantomData<&'a T>
}
