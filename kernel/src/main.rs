#![no_std]
#![no_main]

extern crate alloc;

mod ata;
mod fat32;
mod gdt;
mod idt;
mod ipc;
mod memory;
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
    println!("  Architecture: x86_64");
    println!("===========================================");
    println!();

    // Verify Limine
    if !BASE_REVISION.is_supported() {
        println!("[FATAL] Limine base revision NOT supported!");
        hcf();
    }

    // 2. PCI Bus & Hardware Discovery
    pci::enumerate_pci();

    // 3. GDT
    gdt::init();
    println!("[OK] GDT initialized");

    // 4. IDT (must be initialized before PIC enables IRQs!)
    idt::init();
    println!("[OK] IDT initialized (256 entries + syscall 0x80)");

    // 5. PIC
    pic::init();
    println!("[OK] PIC remapped (IRQs 32-47)");

    // 6. PIT (Timer at 1000 Hz for Preemption)
    pit::init(1000);

    // 7. PS/2 Mouse
    idt::init_mouse();

    // 8. Frame allocator
    let mmap = MEMORY_MAP_REQUEST.get_response()
        .expect("No memory map from Limine");
    memory::FrameAllocator::init(mmap);
    println!("[OK] Frame allocator active");

    // 9. Paging
    let hhdm = HHDM_REQUEST.get_response()
        .expect("No HHDM from Limine");
    memory::paging::init(hhdm.offset());

    // 10. Heap
    memory::heap::init();
    println!("[OK] Heap initialized");

    // 11. Hardware Block Storage (ATA/PIO) & FAT Volume
    ata::init();
    fat32::init();

    // 12. Scheduler
    scheduler::init();

    // 13. Spawn init process
    if let Some(data) = fat32::read_file("init.elf") {
        if let Some(pid) = process::load_elf("init", &data, 0) {
            println!("[OK] Spawned init.elf (PID {})", pid);
        } else {
            println!("[ERROR] Failed to load init.elf");
        }
    } else {
        println!("[ERROR] init.elf not found in root directory!");
    }

    // 14. Enable interrupts
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
