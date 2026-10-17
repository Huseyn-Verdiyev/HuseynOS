#![no_std]
#![no_main]

extern crate alloc;

mod acpi;
mod ahci;
mod apic;
mod ata;
mod fat32;
mod gdt;
mod gpt;
mod idt;
mod ipc;
mod memory;
mod nvme;
mod pic;
mod pit;
mod pci;
mod process;
mod rtc;
mod elf;
mod scheduler;
mod serial;
mod shm;
mod syscall;
mod vfs;
mod vma;
mod xhci;

use core::arch::asm;
use core::panic::PanicInfo;
use limine::request::{
    FramebufferRequest, HhdmRequest, MemoryMapRequest,
    RequestsEndMarker, RequestsStartMarker, StackSizeRequest,
};
use limine::BaseRevision;

// ─── Limine Request Section Markers ───

#[used]
#[unsafe(link_section = ".requests_start_marker")]
static _START_MARKER: RequestsStartMarker = RequestsStartMarker::new();

#[used]
#[unsafe(link_section = ".requests_end_marker")]
static _END_MARKER: RequestsEndMarker = RequestsEndMarker::new();

// ─── Limine Requests ───

#[used]
#[unsafe(link_section = ".requests")]
static BASE_REVISION: BaseRevision = BaseRevision::new();

#[used]
#[unsafe(link_section = ".requests")]
static HHDM_REQUEST: HhdmRequest = HhdmRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static MEMORY_MAP_REQUEST: MemoryMapRequest = MemoryMapRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
pub static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(0x10000);

// ─── Kernel Entry Point ───

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    // 1. Serial output
    serial::init();

    println!("===========================================");
    println!("  HuseynOS v1.0.0 - Monolith Microkernel");
    println!("  Architecture: x86_64 (Bare-Metal Ready)");
    println!("===========================================");
    println!();

    if !BASE_REVISION.is_supported() {
        println!("[FATAL] Limine base revision NOT supported!");
        hcf();
    }

    // 2. GDT
    gdt::init();
    println!("[OK] GDT initialized");

    // 3. IDT (must be initialized before PIC enables IRQs!)
    idt::init();
    println!("[OK] IDT initialized (256 entries + syscall 0x80)");

    // 4. PIC & PIT
    pic::init();
    println!("[OK] PIC remapped (IRQs 32-47)");
    pit::init(1000);

    // 5. PS/2 Mouse
    idt::init_mouse();

    // 6. Frame allocator
    let mmap = MEMORY_MAP_REQUEST.get_response()
        .expect("No memory map from Limine");
    memory::FrameAllocator::init(mmap);
    println!("[OK] Frame allocator active");

    // 7. Paging + PAT Write-Combining
    let hhdm = HHDM_REQUEST.get_response()
        .expect("No HHDM from Limine");
    memory::paging::init(hhdm.offset());

    // 8. Kernel Heap
    memory::heap::init();
    println!("[OK] Heap initialized");

    // 9. ACPI (RSDP/XSDT/MADT) & Local APIC / IOAPIC
    acpi::init();
    apic::init();

    // 10. PCI Bus & Hardware Enumeration
    pci::enumerate_pci();

    // 11. USB 3.0 (xHCI) Host Controller & BIOS Handoff
    if let Some(bar0) = pci::find_xhci_bar0() {
        xhci::init(bar0);
    }

    // 12. Storage Hardware: NVMe M.2 SSD, AHCI SATA 3.0 DMA, ATA/PIO, and GPT/FAT Volume
    if let Some(bar0) = pci::find_nvme_bar0() {
        nvme::init(bar0);
    }
    if let Some(bar5) = pci::find_ahci_bar5() {
        ahci::init(bar5);
    }
    ata::init();
    fat32::init();

    // 13. Scheduler
    scheduler::init();

    // 14. Spawn init process
    if let Some(data) = fat32::read_file("init.elf") {
        if let Some(pid) = process::load_elf("init.elf", &data, 0) {
            println!("[OK] Spawned init.elf (PID {})", pid);
        } else {
            println!("[ERROR] Failed to load init.elf");
        }
    } else {
        println!("[ERROR] init.elf not found in root directory!");
    }

    // 15. Enable interrupts
    idt::enable_interrupts();
    println!("[OK] Interrupts enabled - HuseynOS Monolith ready!");
    println!();

    loop {
        unsafe { asm!("hlt"); }
    }
}

fn hcf() -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!();
    println!("!!! KERNEL PANIC !!!");
    println!("{}", info);
    hcf()
}
