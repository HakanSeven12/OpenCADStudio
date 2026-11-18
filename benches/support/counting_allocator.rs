//! Requested Rust System allocation bytes, excluding allocator overhead and
//! allocations that bypass the global allocator. Realloc counts net live size.

use std::alloc::{GlobalAlloc, Layout, System};
use std::sync::atomic::{AtomicUsize, Ordering};

pub struct CountingAllocator;
static LIVE: AtomicUsize = AtomicUsize::new(0);
static PEAK: AtomicUsize = AtomicUsize::new(0);

fn add(bytes: usize) {
    let now = LIVE.fetch_add(bytes, Ordering::Relaxed) + bytes;
    PEAK.fetch_max(now, Ordering::Relaxed);
}

unsafe impl GlobalAlloc for CountingAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc(layout) };
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn alloc_zeroed(&self, layout: Layout) -> *mut u8 {
        let ptr = unsafe { System.alloc_zeroed(layout) };
        if !ptr.is_null() {
            add(layout.size());
        }
        ptr
    }
    unsafe fn dealloc(&self, ptr: *mut u8, layout: Layout) {
        LIVE.fetch_sub(layout.size(), Ordering::Relaxed);
        unsafe { System.dealloc(ptr, layout) };
    }
    unsafe fn realloc(&self, ptr: *mut u8, layout: Layout, size: usize) -> *mut u8 {
        let new = unsafe { System.realloc(ptr, layout, size) };
        if !new.is_null() {
            if size >= layout.size() {
                add(size - layout.size());
            } else {
                LIVE.fetch_sub(layout.size() - size, Ordering::Relaxed);
            }
        }
        new
    }
}

pub fn live() -> usize {
    LIVE.load(Ordering::Relaxed)
}
pub fn peak() -> usize {
    PEAK.load(Ordering::Relaxed)
}
pub fn reset_peak() -> usize {
    let baseline = live();
    PEAK.store(baseline, Ordering::Relaxed);
    baseline
}

pub fn self_test() {
    let before = reset_peak();
    let mut bytes = Vec::<u8>::with_capacity(32_768);
    std::hint::black_box(&mut bytes);
    assert!(live() >= before + bytes.capacity());
    bytes.reserve_exact(65_536);
    std::hint::black_box(&mut bytes);
    assert!(live() >= before + bytes.capacity());
    assert!(peak() >= live());
    drop(bytes);
    assert_eq!(live(), before);
}
