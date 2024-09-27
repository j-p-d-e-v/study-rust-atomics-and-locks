use std::cell::UnsafeCell;
use std::sync::atomic::{
    fence, AtomicUsize, Ordering
};
use std::ptr::NonNull;
use std::ops::Deref;
use std::mem::ManuallyDrop;
use std::usize;

pub struct Arc<T> {
    ptr: NonNull<ArcData<T>>,
}

impl<T> Arc<T> {
    pub fn new(data:T) -> Arc<T> {
        Arc {
            ptr: NonNull::from(
                Box::leak(
                    Box::new(
                        ArcData {
                            alloc_ref_count: AtomicUsize::new(1),
                            data_ref_count: AtomicUsize::new(1),
                            data: UnsafeCell::new(ManuallyDrop::new(data)),
                        }
                    )
                )
            )
        }
    }

    fn data(&self) -> &ArcData<T> {
        unsafe { self.ptr.as_ref() }
    }

    pub fn downgrade(arc: &Self) -> Weak<T> {
        let mut n = arc.data().alloc_ref_count.load(Ordering::Relaxed);
        loop {
            if n == usize::MAX {
                std::hint::spin_loop();
                continue;
            }
            assert!(n < usize::MAX - 1);
            if let Err(e) = arc.data().alloc_ref_count.compare_exchange_weak(n,n+1, Ordering::Acquire, Ordering::Relaxed) {
                n = e;
                continue;
            }
            return Weak { ptr: arc.ptr }
        }

    }

    pub fn get_mut(arc: &mut Self) -> Option<&mut T> {
        if arc.data().alloc_ref_count.compare_exchange(1,usize::MAX,Ordering::Acquire,Ordering::Relaxed).is_err() {
            return None;
        }
        let is_unique = arc.data().data_ref_count.load(Ordering::Relaxed) == 1;
        arc.data().alloc_ref_count.store(1, Ordering::Release);
        if !is_unique {
            return None;
        }
        fence(Ordering::Acquire);
        unsafe { Some(&mut *arc.data().data.get()) }
    }
}

impl<T> Deref for Arc<T> {
    type Target = T;

    fn deref(&self) -> &T {
        unsafe  { &*self.data().data.get() }
    }
}

impl<T> Clone for Arc<T> {

    fn clone(&self) -> Self {
        if self.data().data_ref_count.fetch_add(1,Ordering::Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Arc {
            ptr: self.ptr
        }
    }
}

impl<T> Drop for Arc<T> {
    fn drop(&mut self) {
        if self.data().data_ref_count.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);
            unsafe {
                ManuallyDrop::drop(&mut *self.data().data.get());
            }
            drop(Weak{ ptr: self.ptr })
        }
    }
}

unsafe impl<T: Send + Sync> Send for Arc<T> {}
unsafe impl<T: Send + Sync> Sync for Arc<T> {}

pub struct Weak<T>{
    ptr: NonNull<ArcData<T>>
}

unsafe impl<T: Send + Sync> Send for Weak<T> {}
unsafe impl<T: Send + Sync> Sync for Weak<T> {}

impl<T> Weak<T> {
    fn data(&self) -> &ArcData<T> {
        unsafe { self.ptr.as_ref() }
    }

    pub fn upgrade(&self) -> Option<Arc<T>> {
        let mut n = self.data().data_ref_count.load(Ordering::Relaxed);
        loop {
            if n == 0 {
                return None;
            }
            assert!(n < usize::MAX);
            if let Err(e) = self.data().data_ref_count.compare_exchange_weak(n,n+1,Ordering::Relaxed, Ordering::Relaxed) {
                n = e;
                continue;
            }
            return Some(Arc { ptr: self.ptr });
        }
    }
}

impl<T> Clone for Weak<T> {

    fn clone(&self) -> Self {
        if self.data().alloc_ref_count.fetch_add(1,Ordering::Relaxed) > usize::MAX / 2 {
            std::process::abort();
        }
        Weak {
            ptr: self.ptr
        }
    }
}
impl<T> Drop for Weak<T> {
    fn drop(&mut self) {

        if self.data().alloc_ref_count.fetch_sub(1, Ordering::Release) == 1 {
            fence(Ordering::Acquire);
            unsafe  {
                drop(Box::from_raw(self.ptr.as_ptr()))
            }
        }
    }
}

struct ArcData<T> {
    data_ref_count: AtomicUsize,
    alloc_ref_count: AtomicUsize,
    data: UnsafeCell<ManuallyDrop<T>>
}


#[test]
fn test() {

    static NUM_DROPS: AtomicUsize = AtomicUsize::new(0);

    struct DetectDrop;

    impl Drop for DetectDrop {
        fn drop(&mut self) {
            NUM_DROPS.fetch_add(1, Ordering::Relaxed);
        }
    }

    let x = Arc::new(("hello",DetectDrop));
    let y = Arc::downgrade(&x);
    let z = Arc::downgrade(&x);

    let t = std::thread::spawn(move || {
        let y = y.upgrade().unwrap();
        assert_eq!(y.0,"hello");
    });

    assert_eq!(x.0,"hello");
    t.join().unwrap();
    assert_eq!(NUM_DROPS.load(Ordering::Relaxed),0);
    assert!(z.upgrade().is_some());

    drop(x);
    assert_eq!(NUM_DROPS.load(Ordering::Relaxed),1);
    assert!(z.upgrade().is_none());
}