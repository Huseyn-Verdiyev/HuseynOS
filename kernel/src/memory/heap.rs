use crate::{serial_print, serial_println};
use crate::memory::frame::FrameAllocator;
use crate::memory::paging;
use linked_list_allocator::LockedHeap;

/// Heap start virtual address (after kernel space).
const HEAP_START: u64 = 0xFFFF_C000_0000_0000;
/// Heap size: 1 MiB.
const HEAP_SIZE: usize = 1024 * 1024;

#[global_allocator]
static ALLOCATOR: LockedHeap = LockedHeap::empty();

/// Initialize the kernel heap.
/// Allocates physical frames and maps them to the heap virtual address region.
pub fn init() {
    let pages_needed = HEAP_SIZE / crate::memory::frame::PAGE_SIZE;

    for i in 0..pages_needed {
        let phys = FrameAllocator::alloc().expect("Out of frames for heap");
        let virt = HEAP_START + (i as u64) * crate::memory::frame::PAGE_SIZE as u64;
        paging::map_page(virt, phys, paging::WRITABLE);
    }

    unsafe {
        ALLOCATOR.lock().init(HEAP_START as *mut u8, HEAP_SIZE);
    }

    serial_println!(
        "[OK] Heap initialized: {} KiB at {:#X}",
        HEAP_SIZE / 1024,
        HEAP_START
    );
}
