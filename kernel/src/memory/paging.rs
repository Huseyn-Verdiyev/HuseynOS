use crate::{serial_print, serial_println};
use crate::memory::frame::{FrameAllocator, PAGE_SIZE};

/// HHDM offset — Limine maps all physical memory at this virtual base.
/// Updated at runtime from Limine's HHDM response.
static mut HHDM_OFFSET: u64 = 0;

/// Page table entry flags.
pub const PRESENT: u64 = 1 << 0;
pub const WRITABLE: u64 = 1 << 1;
pub const USER: u64 = 1 << 2;
pub const NO_EXECUTE: u64 = 1 << 63;

/// Set the HHDM offset (call once from kmain with Limine's HHDM response).
pub fn set_hhdm_offset(offset: u64) {
    unsafe { HHDM_OFFSET = offset; }
}

/// Convert physical address to virtual address using HHDM.
pub fn phys_to_virt(phys: u64) -> u64 {
    unsafe { phys + HHDM_OFFSET }
}

/// Get current PML4 (CR3).
pub fn get_pml4() -> u64 {
    let cr3: u64;
    unsafe { core::arch::asm!("mov {}, cr3", out(reg) cr3); }
    cr3 & !0xFFF // Mask out flags
}

/// Map a virtual page to a physical frame in a specific PML4 table.
pub fn map_page_in_table(pml4_phys: u64, virt_addr: u64, phys_addr: u64, flags: u64) {
    let indices = [
        ((virt_addr >> 39) & 0x1FF) as usize, // PML4
        ((virt_addr >> 30) & 0x1FF) as usize, // PDPT
        ((virt_addr >> 21) & 0x1FF) as usize, // PD
        ((virt_addr >> 12) & 0x1FF) as usize, // PT
    ];

    let mut table_phys = pml4_phys;

    // Walk/create page tables for levels 0-2 (PML4, PDPT, PD)
    for level in 0..3 {
        let table_virt = phys_to_virt(table_phys) as *mut u64;
        let entry = unsafe { table_virt.add(indices[level]).read_volatile() };

        if entry & PRESENT != 0 {
            table_phys = entry & 0x000F_FFFF_FFFF_F000;
        } else {
            // Allocate new page table
            let new_table = FrameAllocator::alloc().expect("Out of frames for page table");
            // Zero the new table
            unsafe {
                let ptr = phys_to_virt(new_table) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
            }
            // Write entry (We use USER flat so user mappings can traverse intermediate tables)
            unsafe {
                table_virt.add(indices[level]).write_volatile(new_table | PRESENT | WRITABLE | USER);
            }
            table_phys = new_table;
        }
    }

    // Level 3: PT — write the final mapping
    let pt_virt = phys_to_virt(table_phys) as *mut u64;
    unsafe {
        pt_virt.add(indices[3]).write_volatile(phys_addr | flags | PRESENT);
    }

    // Invalidate TLB if we are modifying the active table
    if pml4_phys == get_pml4() {
        unsafe {
            core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
        }
    }
}

/// Map a virtual page to a physical frame in the current active page table.
pub fn map_page(virt_addr: u64, phys_addr: u64, flags: u64) {
    map_page_in_table(get_pml4(), virt_addr, phys_addr, flags);
}

/// Create a new PML4 table for a user process.
/// It clones the Higher-Half (kernel space) from the active PML4 so the kernel is still accessible when in ring 0,
/// but leaves the Lower-Half (indexes 0-255) empty for the user process.
pub fn create_user_page_table() -> u64 {
    let new_pml4_phys = FrameAllocator::alloc().expect("Failed to allocate user PML4");
    let new_pml4_virt = phys_to_virt(new_pml4_phys) as *mut [u64; 512];

    let active_pml4_phys = get_pml4();
    let active_pml4_virt = phys_to_virt(active_pml4_phys) as *const [u64; 512];

    unsafe {
        // Zero out the whole new table first
        core::ptr::write_bytes(new_pml4_virt as *mut u8, 0, PAGE_SIZE);

        let active_table = &*active_pml4_virt;
        let new_table = &mut *new_pml4_virt;

        // Copy Higher Half (index 256 to 511)
        for i in 256..512 {
            new_table[i] = active_table[i];
        }
    }

    new_pml4_phys
}

/// Returns the physical frame address currently mapped to the virtual address, if any.
pub fn get_mapped_frame(pml4_phys: u64, virt_addr: u64) -> Option<u64> {
    let indices = [
        ((virt_addr >> 39) & 0x1FF) as usize,
        ((virt_addr >> 30) & 0x1FF) as usize,
        ((virt_addr >> 21) & 0x1FF) as usize,
        ((virt_addr >> 12) & 0x1FF) as usize,
    ];

    let mut table_phys = pml4_phys;

    for level in 0..3 {
        let table_virt = phys_to_virt(table_phys) as *mut u64;
        let entry = unsafe { table_virt.add(indices[level]).read_volatile() };
        if entry & PRESENT == 0 {
            return None;
        }

        // Check the PS (Page Size) bit 7 for huge pages
        if (level == 1 || level == 2) && (entry & 0x80 != 0) {
            let base_phys = entry & 0x000F_FFFF_FFFF_F000;
            if level == 1 {
                // 1GB Huge Page
                let offset = virt_addr & 0x3FFF_FFFF;
                return Some(base_phys + offset);
            } else {
                // 2MB Huge Page
                let offset = virt_addr & 0x1F_FFFF;
                return Some(base_phys + offset);
            }
        }

        table_phys = entry & 0x000F_FFFF_FFFF_F000;
    }

    let pt_virt = phys_to_virt(table_phys) as *mut u64;
    let entry = unsafe { pt_virt.add(indices[3]).read_volatile() };
    if entry & PRESENT == 0 {
        return None;
    }
    Some((entry & 0x000F_FFFF_FFFF_F000) + (virt_addr & 0xFFF))
}

/// Unmap a virtual page.
#[allow(dead_code)]
pub fn unmap_page(virt_addr: u64) {
    let pml4_phys = get_pml4();
    let indices = [
        ((virt_addr >> 39) & 0x1FF) as usize,
        ((virt_addr >> 30) & 0x1FF) as usize,
        ((virt_addr >> 21) & 0x1FF) as usize,
        ((virt_addr >> 12) & 0x1FF) as usize,
    ];

    let mut table_phys = pml4_phys;

    for level in 0..3 {
        let table_virt = phys_to_virt(table_phys) as *mut u64;
        let entry = unsafe { table_virt.add(indices[level]).read_volatile() };
        if entry & PRESENT == 0 {
            return; // Not mapped
        }
        table_phys = entry & 0x000F_FFFF_FFFF_F000;
    }

    let pt_virt = phys_to_virt(table_phys) as *mut u64;
    unsafe {
        pt_virt.add(indices[3]).write_volatile(0);
        core::arch::asm!("invlpg [{}]", in(reg) virt_addr, options(nostack, preserves_flags));
    }
}

/// The Kernel's default page table (Limine's HHDM table).
pub static mut KERNEL_PML4_PHYS: u64 = 0;

/// Initialize paging. Limine already sets up page tables; we just record the HHDM offset.
pub fn init(hhdm_offset: u64) {
    set_hhdm_offset(hhdm_offset);
    unsafe {
        KERNEL_PML4_PHYS = get_pml4();
    }
    serial_println!("[OK] Paging initialized (HHDM offset: {:#X})", hhdm_offset);
}
