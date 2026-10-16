use crate::serial_println;
use crate::memory::frame::{FrameAllocator, PAGE_SIZE};

/// HHDM offset — Limine maps all physical memory at this virtual base.
static mut HHDM_OFFSET: u64 = 0;

/// Page table entry flags.
pub const PRESENT: u64         = 1 << 0;
pub const WRITABLE: u64        = 1 << 1;
pub const USER: u64            = 1 << 2;
/// Page-level Write-Through (mapped to PAT1 = Write-Combining after `init_pat()`).
pub const WRITE_COMBINING: u64 = 1 << 3;
/// Page-level Cache Disable (for MMIO registers like APIC, IOAPIC, AHCI, xHCI).
pub const CACHE_DISABLE: u64   = 1 << 4;
pub const NO_EXECUTE: u64      = 1 << 63;

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
    cr3 & !0xFFF
}

/// Program the x86_64 IA32_PAT MSR (0x277) so PAT Entry 1 (PWT=1, PCD=0, PAT=0) is Write-Combining (0x01).
fn init_pat() {
    const IA32_PAT_MSR: u32 = 0x277;
    unsafe {
        let low: u32;
        let high: u32;
        core::arch::asm!(
            "rdmsr",
            in("ecx") IA32_PAT_MSR,
            out("eax") low,
            out("edx") high,
            options(nomem, nostack, preserves_flags)
        );
        // Replace PA1 (bits 8..15 of EAX) with 0x01 (Write-Combining)
        let new_low = (low & !(0xFF << 8)) | (0x01 << 8);
        core::arch::asm!(
            "wrmsr",
            in("ecx") IA32_PAT_MSR,
            in("eax") new_low,
            in("edx") high,
            options(nomem, nostack, preserves_flags)
        );
    }
}

/// Map a virtual page to a physical frame in a specific PML4 table.
pub fn map_page_in_table(pml4_phys: u64, virt_addr: u64, phys_addr: u64, flags: u64) {
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

        if entry & PRESENT != 0 {
            table_phys = entry & 0x000F_FFFF_FFFF_F000;
        } else {
            let new_table = FrameAllocator::alloc().expect("Out of frames for page table");
            unsafe {
                let ptr = phys_to_virt(new_table) as *mut u8;
                core::ptr::write_bytes(ptr, 0, PAGE_SIZE);
                table_virt.add(indices[level]).write_volatile(new_table | PRESENT | WRITABLE | USER);
            }
            table_phys = new_table;
        }
    }

    let pt_virt = phys_to_virt(table_phys) as *mut u64;
    unsafe {
        pt_virt.add(indices[3]).write_volatile(phys_addr | flags | PRESENT);
    }

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
pub fn create_user_page_table() -> u64 {
    let new_pml4_phys = FrameAllocator::alloc().expect("Failed to allocate user PML4");
    let new_pml4_virt = phys_to_virt(new_pml4_phys) as *mut [u64; 512];

    let active_pml4_phys = get_pml4();
    let active_pml4_virt = phys_to_virt(active_pml4_phys) as *const [u64; 512];

    unsafe {
        core::ptr::write_bytes(new_pml4_virt as *mut u8, 0, PAGE_SIZE);

        let active_table = &*active_pml4_virt;
        let new_table = &mut *new_pml4_virt;

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

        if (level == 1 || level == 2) && (entry & 0x80 != 0) {
            let base_phys = entry & 0x000F_FFFF_FFFF_F000;
            if level == 1 {
                let offset = virt_addr & 0x3FFF_FFFF;
                return Some(base_phys + offset);
            } else {
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
            return;
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

/// Initialize paging and hardware PAT Write-Combining.
pub fn init(hhdm_offset: u64) {
    set_hhdm_offset(hhdm_offset);
    init_pat();
    unsafe {
        KERNEL_PML4_PHYS = get_pml4();
    }
    serial_println!("[OK] Paging initialized (HHDM: {:#X}, PAT1=Write-Combining)", hhdm_offset);
}
