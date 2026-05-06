use alloc::vec::Vec;
use spin::Mutex;
use crate::memory::frame::{FrameAllocator, PAGE_SIZE};

/// Maximum number of shared memory regions.
const MAX_SHM_REGIONS: usize = 32;

/// A shared memory region that can be mapped by multiple processes.
#[derive(Debug)]
pub struct SharedRegion {
    /// Unique ID of this SHM region.
    pub id: u32,
    /// Physical frames backing this region.
    pub frames: Vec<u64>,
    /// Size in bytes (page-aligned).
    pub size: usize,
    /// Number of processes currently mapping this region.
    pub ref_count: usize,
    /// PID of the process that created this region.
    pub owner: usize,
}

/// Global shared memory table.
static SHM_TABLE: Mutex<Vec<SharedRegion>> = Mutex::new(Vec::new());
static NEXT_SHM_ID: Mutex<u32> = Mutex::new(1);

/// Create a new shared memory region.
/// Allocates physical frames immediately.
/// Returns the SHM ID, or None if out of memory.
pub fn create(size: usize, owner_pid: usize) -> Option<u32> {
    let aligned_size = (size + PAGE_SIZE - 1) & !(PAGE_SIZE - 1);
    let num_pages = aligned_size / PAGE_SIZE;

    let mut frames = Vec::with_capacity(num_pages);
    for _ in 0..num_pages {
        if let Some(frame) = FrameAllocator::alloc() {
            // Zero the frame
            unsafe {
                let ptr = crate::memory::paging::phys_to_virt(frame) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }
            frames.push(frame);
        } else {
            // Out of memory — free already allocated frames
            // (We don't have frame free yet, so just fail)
            return None;
        }
    }

    let mut id_lock = NEXT_SHM_ID.lock();
    let id = *id_lock;
    *id_lock += 1;
    drop(id_lock);

    let region = SharedRegion {
        id,
        frames,
        size: aligned_size,
        ref_count: 0,
        owner: owner_pid,
    };

    let mut table = SHM_TABLE.lock();
    if table.len() >= MAX_SHM_REGIONS {
        return None;
    }
    table.push(region);

    Some(id)
}

/// Map a shared memory region into a process's address space.
/// `pml4_phys` is the process's PML4 table.
/// `virt_addr` is where to map the SHM in the process's virtual space.
/// Returns 0 on success, or an error code.
pub fn map(shm_id: u32, pml4_phys: u64, virt_addr: u64) -> Result<usize, ()> {
    let mut table = SHM_TABLE.lock();
    if let Some(region) = table.iter_mut().find(|r| r.id == shm_id) {
        let page_aligned_virt = virt_addr & !0xFFF;
        for (i, &frame) in region.frames.iter().enumerate() {
            let page_virt = page_aligned_virt + (i * PAGE_SIZE) as u64;
            crate::memory::paging::map_page_in_table(
                pml4_phys,
                page_virt,
                frame,
                crate::memory::paging::WRITABLE | crate::memory::paging::USER,
            );
        }
        region.ref_count += 1;
        Ok(region.size)
    } else {
        Err(())
    }
}

/// Unmap a shared memory region from a process.
pub fn unmap(shm_id: u32, pml4_phys: u64, virt_addr: u64) -> Result<(), ()> {
    let mut table = SHM_TABLE.lock();
    if let Some(region) = table.iter_mut().find(|r| r.id == shm_id) {
        let page_aligned_virt = virt_addr & !0xFFF;
        for i in 0..region.frames.len() {
            let page_virt = page_aligned_virt + (i * PAGE_SIZE) as u64;
            crate::memory::paging::unmap_page(page_virt);
        }
        if region.ref_count > 0 {
            region.ref_count -= 1;
        }
        Ok(())
    } else {
        Err(())
    }
}
