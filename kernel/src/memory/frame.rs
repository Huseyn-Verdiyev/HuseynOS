use crate::{serial_print, serial_println};
use limine::memory_map::EntryType;
use limine::response::MemoryMapResponse;
use spin::Mutex;

/// Page/frame size: 4 KiB.
pub const PAGE_SIZE: usize = 4096;

/// Maximum supported physical memory: 4 GiB (for bitmap sizing).
const MAX_PHYS_MEMORY: usize = 4 * 1024 * 1024 * 1024;
const MAX_FRAMES: usize = MAX_PHYS_MEMORY / PAGE_SIZE;
/// Bitmap: 1 bit per frame. MAX_FRAMES / 8 = 131072 bytes = 128 KiB.
const BITMAP_SIZE: usize = MAX_FRAMES / 8;

static FRAME_ALLOC: Mutex<FrameAllocatorInner> = Mutex::new(FrameAllocatorInner::new());

struct FrameAllocatorInner {
    bitmap: [u8; BITMAP_SIZE],
    total_frames: usize,
    used_frames: usize,
}

impl FrameAllocatorInner {
    const fn new() -> Self {
        Self {
            bitmap: [0xFF; BITMAP_SIZE], // All marked as used initially
            total_frames: 0,
            used_frames: 0,
        }
    }

    fn mark_free(&mut self, frame: usize) {
        let byte = frame / 8;
        let bit = frame % 8;
        if byte < BITMAP_SIZE {
            self.bitmap[byte] &= !(1 << bit);
        }
    }

    fn mark_used(&mut self, frame: usize) {
        let byte = frame / 8;
        let bit = frame % 8;
        if byte < BITMAP_SIZE {
            self.bitmap[byte] |= 1 << bit;
        }
    }

    fn is_free(&self, frame: usize) -> bool {
        let byte = frame / 8;
        let bit = frame % 8;
        if byte < BITMAP_SIZE {
            self.bitmap[byte] & (1 << bit) == 0
        } else {
            false
        }
    }

    fn alloc(&mut self) -> Option<u64> {
        for byte_idx in 0..BITMAP_SIZE {
            if self.bitmap[byte_idx] != 0xFF {
                for bit in 0..8 {
                    let frame = byte_idx * 8 + bit;
                    if self.is_free(frame) {
                        self.mark_used(frame);
                        self.used_frames += 1;
                        return Some((frame as u64) * PAGE_SIZE as u64);
                    }
                }
            }
        }
        None
    }

    fn dealloc(&mut self, phys_addr: u64) {
        let frame = (phys_addr / PAGE_SIZE as u64) as usize;
        if !self.is_free(frame) {
            self.mark_free(frame);
            if self.used_frames > 0 {
                self.used_frames -= 1;
            }
        }
    }
}

/// Public frame allocator API.
pub struct FrameAllocator;

impl FrameAllocator {
    /// Initialize from Limine memory map, marking usable regions as free.
    pub fn init(memory_map: &MemoryMapResponse) {
        let mut alloc = FRAME_ALLOC.lock();
        let mut free_count = 0usize;

        for entry in memory_map.entries() {
            if entry.entry_type == EntryType::USABLE {
                let start_frame = (entry.base as usize + PAGE_SIZE - 1) / PAGE_SIZE;
                let end_frame = ((entry.base + entry.length) as usize) / PAGE_SIZE;

                for frame in start_frame..end_frame {
                    if frame < MAX_FRAMES {
                        alloc.mark_free(frame);
                        free_count += 1;
                    }
                }
            }
        }

        alloc.total_frames = free_count;
        alloc.used_frames = 0;

        serial_println!(
            "[OK] Frame allocator: {} free frames ({} MiB usable RAM)",
            free_count,
            (free_count * PAGE_SIZE) / (1024 * 1024)
        );
    }

    /// Allocate a single 4 KiB frame. Returns physical address.
    pub fn alloc() -> Option<u64> {
        FRAME_ALLOC.lock().alloc()
    }

    /// Deallocate a 4 KiB frame by physical address.
    pub fn dealloc(phys_addr: u64) {
        FRAME_ALLOC.lock().dealloc(phys_addr);
    }

    /// Get count of free frames.
    pub fn free_count() -> usize {
        let alloc = FRAME_ALLOC.lock();
        alloc.total_frames - alloc.used_frames
    }
}
