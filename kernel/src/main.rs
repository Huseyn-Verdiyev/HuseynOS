#![no_std]
#![no_main]

extern crate alloc;

mod console;
mod fat32;
mod gdt;
mod idt;
mod ipc;
mod keyboard;
mod memory;
mod pic;
mod pit;
mod process;
mod rtc;
mod scheduler;
mod serial;
mod shell;
mod syscall;
mod vga_font;

use alloc::vec;
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
static FRAMEBUFFER_REQUEST: FramebufferRequest = FramebufferRequest::new();

#[used]
#[unsafe(link_section = ".requests")]
static STACK_SIZE_REQUEST: StackSizeRequest = StackSizeRequest::new().with_size(0x10000);

// No inline test tasks here anymore, using shell::shell_task

// ─── Kernel Entry Point ───

#[unsafe(no_mangle)]
unsafe extern "C" fn kmain() -> ! {
    // 1. Serial output
    serial::init();
    // Initialize Console if framebuffer is available
    if let Some(response) = FRAMEBUFFER_REQUEST.get_response() {
        if let Some(fb) = response.framebuffers().next() {
            console::CONSOLE.lock().init(&fb);
        }
    }

    println!("===========================================");
    println!("  HuseynOS v0.4.0 - CLI & Drivers");
    println!("  Architecture: x86_64");
    println!("===========================================");
    println!();

    // Verify Limine
    if !BASE_REVISION.is_supported() {
        println!("[FATAL] Limine base revision NOT supported!");
        hcf();
    }

    // 2. GDT
    gdt::init();
    println!("[OK] GDT initialized");

    // 3. PIC
    pic::init();
    println!("[OK] PIC remapped (IRQs 32-47)");

    // 3.5. PIT (Timer at 1000 Hz for Preemption)
    pit::init(1000);
    idt::init();
    println!("[OK] IDT initialized (256 entries + syscall 0x80)");

    // 4. FAT32 File System (via Ramdisk)
    fat32::init();

    // 4. Frame allocator
    let mmap = MEMORY_MAP_REQUEST.get_response()
        .expect("No memory map from Limine");
    memory::FrameAllocator::init(mmap);
    println!("[OK] Frame allocator active");

    // 5. Paging
    let hhdm = HHDM_REQUEST.get_response()
        .expect("No HHDM from Limine");
    memory::paging::init(hhdm.offset());

    // 6. Heap
    memory::heap::init();
    println!("[OK] Heap initialized");

    // 7. Keyboard
    keyboard::init();
    println!("[OK] Keyboard initialized");

    // 7. Scheduler
    scheduler::init();

    // 8. Spawn shell task
    let pid_shell = process::spawn("Shell", shell::shell_task);
    println!("[OK] Spawned Shell Task (PID {})", pid_shell);

    // 9. Enable interrupts
    idt::enable_interrupts();
    println!("[OK] Interrupts enabled - OS ready!");
    println!();

    // Idle loop — PIT preemption handles scheduling automatically.
    // We just hlt to save power; the timer will wake us and schedule tasks.
    loop {
        unsafe { asm!("hlt"); }
    }
}

// ─── Halt ───

fn hcf() -> ! {
    loop {
        unsafe { asm!("hlt"); }
    }
}

// ─── Panic Handler ───

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    println!();
    println!("!!! KERNEL PANIC !!!");
    println!("{}", info);
    hcf()
}
