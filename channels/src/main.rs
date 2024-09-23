use std::mem::MaybeUninit;
use std::cell::UnsafeCell;
use std::sync::atomic::{
    AtomicU8,
    Ordering::{
        Release,
        Acquire,
        Relaxed
    }
};
use std::marker::Sync;

const EMPTY: u8 = 0;
const WRITING: u8 = 1;
const READY: u8 = 2;
const READING: u8 = 3;

#[derive(Debug)]
pub struct Channel<T> {
    message: UnsafeCell<MaybeUninit<T>>,
    state: AtomicU8
}

unsafe impl <T> Sync for Channel<T> where T: Send {}

impl<T> Drop for Channel<T> {
    fn drop(&mut self) {
        if *self.state.get_mut() == READY {
            unsafe { self.message.get_mut().assume_init_drop() };
        }
    }
}

impl<T> Channel<T> {
    pub fn new() -> Self {
        Self {
            message: UnsafeCell::new(MaybeUninit::uninit()),
            state: AtomicU8::new(EMPTY),
        }
    }

    /// Safety: Only call this once!
    pub fn send(&self, message: T) {
        if self.state.compare_exchange(EMPTY,WRITING, Release, Relaxed).is_err() {
            panic!("cant't send more than one message!");
        }
        unsafe { (*self.message.get()).write(message) };
        self.state.store(READY, Release);
    }

    pub fn is_ready(&self) -> bool {
        self.state.load(Relaxed) == READY
    }
    
    /// Safety: Only call this once
    /// and only after is_ready() returns true
    pub fn receive(&self) -> T {
        if self.state.compare_exchange(READY,READING,Acquire,Relaxed).is_err() {
            panic!("no message available!");
        }
        unsafe { (*self.message.get()).assume_init_read() }
    }
}


fn main() {

    let channel: Channel<String> = Channel::new();
    let t = std::thread::current();

    std::thread::scope(|s| {
        s.spawn(|| {
            channel.send("Hello there!".to_string());
            t.unpark();
        });

        while !channel.is_ready() {
            std::thread::park();
        }
        assert_eq!(channel.receive(),"Hello there!".to_string());
    });
}
