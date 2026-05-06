//! Simple bump allocator for userland processes using sbrk().
//! Implements the `GlobalAlloc` trait so that `alloc::vec::Vec`,
//! `alloc::string::String`, `alloc::boxed::Box` all work automatically.

use core::alloc::{GlobalAlloc, Layout};
use core::sync::atomic::{AtomicUsize, Ordering};

/// A simple bump allocator backed by sbrk().
///
/// Allocations bump forward. Free is a no-op (memory is only reclaimed
/// when the process exits). This is fine for small userland programs.
pub struct BumpAllocator {
    heap_start: AtomicUsize,
    heap_end: AtomicUsize,
}

impl BumpAllocator {
    pub const fn new() -> Self {
        BumpAllocator {
            heap_start: AtomicUsize::new(0),
            heap_end: AtomicUsize::new(0),
        }
    }
}

unsafe impl GlobalAlloc for BumpAllocator {
    unsafe fn alloc(&self, layout: Layout) -> *mut u8 {
        let size = layout.size();
        let align = layout.align();

        loop {
            let current_start = self.heap_start.load(Ordering::Relaxed);
            let current_end = self.heap_end.load(Ordering::Relaxed);

            // First allocation — initialize the heap
            if current_end == 0 {
                let chunk = 64 * 1024; // Initial 64 KiB heap
                let base = crate::sbrk(chunk);
                if base.is_null() {
                    return core::ptr::null_mut();
                }
                let base_addr = base as usize;
                self.heap_start.store(base_addr, Ordering::Release);
                self.heap_end.store(base_addr + chunk, Ordering::Release);
                continue; // retry with initialized heap
            }

            // Align the current start
            let aligned = (current_start + align - 1) & !(align - 1);
            let new_start = aligned + size;

            if new_start <= current_end {
                // Enough space — bump the pointer
                if self.heap_start.compare_exchange(
                    current_start, new_start, Ordering::AcqRel, Ordering::Relaxed
                ).is_ok() {
                    return aligned as *mut u8;
                }
                // CAS failed — retry
                continue;
            }

            // Not enough space — request more from kernel
            let needed = new_start - current_end;
            let chunk = if needed < 64 * 1024 { 64 * 1024 } else { needed };
            let result = crate::sbrk(chunk);
            if result.is_null() {
                return core::ptr::null_mut();
            }
            self.heap_end.fetch_add(chunk, Ordering::Release);
            // Retry the allocation with more space
        }
    }

    unsafe fn dealloc(&self, _ptr: *mut u8, _layout: Layout) {
        // Bump allocator doesn't free individual allocations.
        // Memory is reclaimed when the process exits.
    }
}

#[global_allocator]
static ALLOCATOR: BumpAllocator = BumpAllocator::new();
