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
    /// True if the owner process has exited.
    pub owner_exited: bool,
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
            unsafe {
                let ptr = crate::memory::paging::phys_to_virt(frame) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }
            frames.push(frame);
        } else {
            for f in frames {
                FrameAllocator::dealloc(f);
            }
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
        owner_exited: false,
    };

    let mut table = SHM_TABLE.lock();
    if table.len() >= MAX_SHM_REGIONS {
        for &f in &region.frames {
            FrameAllocator::dealloc(f);
        }
        return None;
    }
    table.push(region);

    Some(id)
}

/// Map a shared memory region into a process's address space.
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

/// Unmap a shared memory region from a process and reclaim physical frames if unused.
pub fn unmap(shm_id: u32, _pml4_phys: u64, virt_addr: u64) -> Result<(), ()> {
    let mut table = SHM_TABLE.lock();
    if let Some(idx) = table.iter().position(|r| r.id == shm_id) {
        let page_aligned_virt = virt_addr & !0xFFF;
        for i in 0..table[idx].frames.len() {
            let page_virt = page_aligned_virt + (i * PAGE_SIZE) as u64;
            crate::memory::paging::unmap_page(page_virt);
        }
        if table[idx].ref_count > 0 {
            table[idx].ref_count -= 1;
        }
        // Reclaim physical frames if no mappings remain or owner has exited
        if table[idx].ref_count <= 1 && table[idx].owner_exited {
            let removed = table.remove(idx);
            for frame in removed.frames {
                FrameAllocator::dealloc(frame);
            }
        }
        Ok(())
    } else {
        Err(())
    }
}

/// Called when a process exits to mark its owned SHM regions and free unreferenced frames.
pub fn cleanup_process(pid: usize) {
    let mut table = SHM_TABLE.lock();
    let mut i = 0;
    while i < table.len() {
        if table[i].owner == pid {
            table[i].owner_exited = true;
            if table[i].ref_count > 0 {
                table[i].ref_count -= 1;
            }
            if table[i].ref_count == 0 {
                let removed = table.remove(i);
                for frame in removed.frames {
                    FrameAllocator::dealloc(frame);
                }
                continue;
            }
        }
        i += 1;
    }
}
