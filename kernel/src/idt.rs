use spin::Mutex;
use crate::{serial_print, serial_println};
use core::arch::{asm, naked_asm};

use crate::gdt;
use crate::pic;
use crate::scheduler;
use crate::syscall;
use crate::process::InterruptContext;

/// IDT Entry — 16-byte interrupt gate descriptor for x86_64.
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct IdtEntry {
    offset_low: u16,
    selector: u16,
    ist: u8,
    type_attr: u8,
    offset_mid: u16,
    offset_high: u32,
    _reserved: u32,
}

impl IdtEntry {
    const fn missing() -> Self {
        Self {
            offset_low: 0,
            selector: 0,
            ist: 0,
            type_attr: 0,
            offset_mid: 0,
            offset_high: 0,
            _reserved: 0,
        }
    }

    fn set_handler(&mut self, handler: u64, ist_index: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector = gdt::KERNEL_CODE_SEL;
        self.ist = ist_index;
        self.type_attr = 0x8E; // Present, DPL=0, Interrupt Gate (64-bit)
        self._reserved = 0;
    }

    fn set_handler_user(&mut self, handler: u64, ist_index: u8) {
        self.offset_low = handler as u16;
        self.offset_mid = (handler >> 16) as u16;
        self.offset_high = (handler >> 32) as u32;
        self.selector = gdt::KERNEL_CODE_SEL;
        self.ist = ist_index;
        self.type_attr = 0xEE; // Present, DPL=3, Interrupt Gate (64-bit) - Accessible from Ring 3
        self._reserved = 0;
    }
}

#[repr(C, packed)]
struct IdtPointer {
    limit: u16,
    base: u64,
}

static mut IDT: [IdtEntry; 256] = [IdtEntry::missing(); 256];

// ─── ISR stubs (no error code) ───

macro_rules! isr_no_err {
    ($name:ident, $num:expr) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            naked_asm!(
                "push 0",
                "push {}",
                "jmp isr_common",
                const $num,
            );
        }
    };
}

macro_rules! isr_err {
    ($name:ident, $num:expr) => {
        #[unsafe(naked)]
        unsafe extern "C" fn $name() {
            naked_asm!(
                "push {}",
                "jmp isr_common",
                const $num,
            );
        }
    };
}

// CPU Exceptions
isr_no_err!(isr0, 0);
isr_no_err!(isr1, 1);
isr_no_err!(isr2, 2);
isr_no_err!(isr3, 3);
isr_no_err!(isr4, 4);
isr_no_err!(isr5, 5);
isr_no_err!(isr6, 6);
isr_no_err!(isr7, 7);
isr_err!(isr8, 8);
isr_no_err!(isr9, 9);
isr_err!(isr10, 10);
isr_err!(isr11, 11);
isr_err!(isr12, 12);
isr_err!(isr13, 13);
isr_err!(isr14, 14);
isr_no_err!(isr15, 15);
isr_no_err!(isr16, 16);
isr_err!(isr17, 17);
isr_no_err!(isr18, 18);
isr_no_err!(isr19, 19);

// IRQs
isr_no_err!(irq0, 32);
isr_no_err!(irq1, 33);
isr_no_err!(irq12, 44); // PS/2 Mouse

// Syscall
isr_no_err!(isr_syscall, 0x80);

/// Common ISR handler — saves registers, calls Rust handler, restores, irets.
#[unsafe(naked)]
#[no_mangle]
unsafe extern "C" fn isr_common() {
    naked_asm!(
        "push rax",
        "push rcx",
        "push rdx",
        "push rbx",
        "push rbp",
        "push rsi",
        "push rdi",
        "push r8",
        "push r9",
        "push r10",
        "push r11",
        "push r12",
        "push r13",
        "push r14",
        "push r15",

        // Save DS and ES
        "mov ax, ds",
        "push rax",
        "mov ax, es",
        "push rax",

        // Load Kernel Data Segment
        "mov ax, 0x10",
        "mov ds, ax",
        "mov es, ax",

        "mov rdi, rsp", // Pass pointer to InterruptContext as the first argument
        "call {handler}",
        "mov rsp, rax", // Update RSP with returned context

        // Restore DS and ES
        "pop rax",
        "mov es, ax",
        "pop rax",
        "mov ds, ax",

        "pop r15",
        "pop r14",
        "pop r13",
        "pop r12",
        "pop r11",
        "pop r10",
        "pop r9",
        "pop r8",
        "pop rdi",
        "pop rsi",
        "pop rbp",
        "pop rbx",
        "pop rdx",
        "pop rcx",
        "pop rax",

        "add rsp, 16", // Skip int_num and error_code
        "iretq",
        handler = sym interrupt_handler,
    );
}

/// Rust interrupt handler. Returns the new RSP (context).
extern "C" fn interrupt_handler(context_ptr: *mut InterruptContext) -> *mut InterruptContext {
    let ctx = unsafe { &mut *context_ptr };
    let int_num = ctx.int_num;
    let error_code = ctx.error_code;

    if int_num < 32 {
        unsafe {
            crate::serial::SERIAL1.force_unlock();
        }
    }

    match int_num {
        0 => {
            serial_println!("[EXCEPTION] Division by zero!");
            loop { unsafe { asm!("hlt"); } }
        }
        6 => {
            serial_println!("[EXCEPTION] Invalid opcode!");
            loop { unsafe { asm!("hlt"); } }
        }
        8 => {
            serial_println!("[EXCEPTION] Double fault! error={}", error_code);
            loop { unsafe { asm!("hlt"); } }
        }
        13 => {
            serial_println!("[EXCEPTION] GPF! error={:#X}", error_code);
            loop { unsafe { asm!("hlt"); } }
        }
        14 => {
            let cr2: u64;
            unsafe { asm!("mov {}, cr2", out(reg) cr2); }
            
            // Check if this is a user-mode page fault (bit 2 of error_code)
            let is_user = error_code & 0x4 != 0;
            
            if is_user {
                // Demand paging: look up VMA for the faulting address
                let pid = crate::scheduler::current_pid();
                let fault_page = cr2 & !0xFFF;
                
                let handled = if let Some(proc) = crate::process::get_process_mut(pid) {
                    if proc.vmas.find(cr2).is_some() {
                        // Valid VMA — allocate a frame and map it
                        if let Some(frame) = crate::memory::frame::FrameAllocator::alloc() {
                            // Zero the frame
                            unsafe {
                                let ptr = crate::memory::paging::phys_to_virt(frame) as *mut u8;
                                core::ptr::write_bytes(ptr, 0, crate::memory::frame::PAGE_SIZE);
                            }
                            // Map into the process's page table
                            crate::memory::paging::map_page_in_table(
                                proc.pml4_phys,
                                fault_page,
                                frame,
                                crate::memory::paging::WRITABLE | crate::memory::paging::USER,
                            );
                            true
                        } else {
                            serial_println!("[FAULT] Out of memory for demand page @ {:#X} (PID {})", cr2, pid);
                            false
                        }
                    } else {
                        serial_println!("[FAULT] Segfault: PID {} accessed {:#X} (no VMA), error={:#X}", pid, cr2, error_code);
                        false
                    }
                } else {
                    false
                };
                
                if handled {
                    // Resume the faulting instruction — it will succeed now
                    return context_ptr;
                } else {
                    // Kill the process
                    crate::process::exit_process(pid, u64::MAX);
                    return scheduler::schedule(context_ptr);
                }
            } else {
                // Kernel page fault — unrecoverable
                serial_println!("[EXCEPTION] Kernel Page fault @ {:#X}, error={:#X}", cr2, error_code);
                loop { unsafe { asm!("hlt"); } }
            }
        }
        32 => {
            pic::send_eoi(0);
            return scheduler::schedule(context_ptr);
        }
        33 => {
            let scancode: u8;
            unsafe {
                core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") scancode, options(nomem, nostack));
            }
            // Send MSG_HARDWARE_INTERRUPT to KEYBOARD_PID (PID 3)
            crate::ipc::send(3, 0x30, scancode as u64, 0, 0, 0, 0, 0);
            pic::send_eoi(1);
        }
        0x80 => {
            // Route all syscalls through syscall::handle
            return syscall::handle(context_ptr);
        }
        44 => {
            // IRQ 12 — PS/2 Mouse
            let data: u8;
            unsafe {
                core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") data, options(nomem, nostack));
            }
            // Send MSG_MOUSE_PACKET to mouse driver (PID 4)
            // Spawn order: init=1, console=2, keyboard=3, mouse=4, compositor=5, terminal=6
            crate::ipc::send(4, 0x40, data as u64, 0, 0, 0, 0, 0);
            pic::send_eoi(12);
        }
        _ => {
            if int_num >= 32 && int_num < 48 {
                pic::send_eoi((int_num - 32) as u8);
            }
        }
    }
    context_ptr
}

/// Initialize the IDT.
pub fn init() {
    unsafe {
        IDT[0].set_handler(isr0 as u64, 0);
        IDT[1].set_handler(isr1 as u64, 0);
        IDT[2].set_handler(isr2 as u64, 0);
        IDT[3].set_handler(isr3 as u64, 0);
        IDT[4].set_handler(isr4 as u64, 0);
        IDT[5].set_handler(isr5 as u64, 0);
        IDT[6].set_handler(isr6 as u64, 0);
        IDT[7].set_handler(isr7 as u64, 0);
        IDT[8].set_handler(isr8 as u64, 1); // Double fault → IST1
        IDT[9].set_handler(isr9 as u64, 0);
        IDT[10].set_handler(isr10 as u64, 0);
        IDT[11].set_handler(isr11 as u64, 0);
        IDT[12].set_handler(isr12 as u64, 0);
        IDT[13].set_handler(isr13 as u64, 0);
        IDT[14].set_handler(isr14 as u64, 0);
        IDT[15].set_handler(isr15 as u64, 0);
        IDT[16].set_handler(isr16 as u64, 0);
        IDT[17].set_handler(isr17 as u64, 0);
        IDT[18].set_handler(isr18 as u64, 0);
        IDT[19].set_handler(isr19 as u64, 0);
        IDT[32].set_handler(irq0 as *const () as u64, 0);
        IDT[33].set_handler(irq1 as *const () as u64, 0);
        IDT[44].set_handler(irq12 as *const () as u64, 0); // PS/2 Mouse
        IDT[0x80].set_handler_user(isr_syscall as *const () as u64, 0);

        let idtr = IdtPointer {
            limit: (core::mem::size_of_val(&IDT) - 1) as u16,
            base: IDT.as_ptr() as u64,
        };

        asm!(
            "lidt [{}]",
            in(reg) &idtr,
            options(readonly, nostack, preserves_flags)
        );
    }
}

/// Enable hardware interrupts.
pub fn enable_interrupts() {
    unsafe { asm!("sti", options(nomem, nostack)); }
}

/// Initialize the PS/2 mouse controller.
pub fn init_mouse() {
    unsafe {
        // Helper: wait until controller input buffer is clear
        fn wait_write() {
            for _ in 0..100_000 {
                let status: u8;
                unsafe {
                    core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") status, options(nomem, nostack));
                }
                if status & 0x02 == 0 { return; }
            }
        }
        // Helper: wait until controller output buffer has data
        fn wait_read() {
            for _ in 0..100_000 {
                let status: u8;
                unsafe {
                    core::arch::asm!("in al, dx", in("dx") 0x64u16, out("al") status, options(nomem, nostack));
                }
                if status & 0x01 != 0 { return; }
            }
        }
        // Helper: read from data port
        fn read_data() -> u8 {
            wait_read();
            let val: u8;
            unsafe {
                core::arch::asm!("in al, dx", in("dx") 0x60u16, out("al") val, options(nomem, nostack));
            }
            val
        }
        // Helper: write to command port
        fn write_cmd(cmd: u8) {
            wait_write();
            unsafe {
                core::arch::asm!("out dx, al", in("dx") 0x64u16, in("al") cmd, options(nomem, nostack));
            }
        }
        // Helper: write to data port
        fn write_data(data: u8) {
            wait_write();
            unsafe {
                core::arch::asm!("out dx, al", in("dx") 0x60u16, in("al") data, options(nomem, nostack));
            }
        }

        // 1. Enable auxiliary device (mouse)
        write_cmd(0xA8);

        // 2. Enable IRQ12 — read config byte, set bit 1, write back
        write_cmd(0x20); // Read command byte
        let mut config = read_data();
        config |= 0x02;  // Enable IRQ 12 (aux interrupt)
        config &= !0x20; // Clear "disable mouse clock" bit
        write_cmd(0x60); // Write command byte
        write_data(config);

        // 3. Tell controller: "next byte goes to mouse"
        write_cmd(0xD4);
        write_data(0xFF); // Reset mouse
        let _ack = read_data(); // ACK (0xFA)
        let _self_test = read_data(); // Self-test result (0xAA)
        let _id = read_data(); // Mouse ID (0x00)

        // 4. Set defaults
        write_cmd(0xD4);
        write_data(0xF6); // Set defaults
        let _ack2 = read_data();

        // 5. Enable data reporting
        write_cmd(0xD4);
        write_data(0xF4); // Enable
        let _ack3 = read_data();

        crate::serial_println!("[OK] PS/2 mouse initialized");
    }
}
