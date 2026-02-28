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

        "mov rdi, rsp", // Pass pointer to InterruptContext as the first argument
        "call {handler}",
        "mov rsp, rax", // Update RSP with returned context

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
            serial_println!("[EXCEPTION] Page fault @ {:#X}, error={:#X}", cr2, error_code);
            loop { unsafe { asm!("hlt"); } }
        }
        32 => {
            pic::send_eoi(0);
            return scheduler::schedule(context_ptr);
        }
        33 => {
            let scancode: u8;
            unsafe {
                asm!("in al, dx", in("dx") 0x60u16, out("al") scancode, options(nomem, nostack));
            }
            crate::keyboard::push_scancode(scancode);
            pic::send_eoi(1);
        }
        0x80 => {
            // Route all syscalls through syscall::handle
            return syscall::handle(context_ptr);
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
        IDT[32].set_handler(irq0 as u64, 0);
        IDT[33].set_handler(irq1 as u64, 0);
        IDT[0x80].set_handler(isr_syscall as u64, 0);

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
