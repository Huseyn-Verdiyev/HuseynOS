use core::arch::asm;

/// Command and data ports for the two 8259 PICs.
const PIC1_CMD: u16 = 0x20;
const PIC1_DATA: u16 = 0x21;
const PIC2_CMD: u16 = 0xA0;
const PIC2_DATA: u16 = 0xA1;

/// ICW1: Initialization
const ICW1_INIT: u8 = 0x10;
const ICW1_ICW4: u8 = 0x01;
/// ICW4: 8086/88 mode
const ICW4_8086: u8 = 0x01;

/// IRQ offset for master PIC (IRQ 0-7 → INT 32-39).
pub const PIC1_OFFSET: u8 = 32;
/// IRQ offset for slave PIC (IRQ 8-15 → INT 40-47).
pub const PIC2_OFFSET: u8 = 40;

/// Write a byte to an I/O port.
unsafe fn outb(port: u16, value: u8) {
    asm!("out dx, al", in("dx") port, in("al") value, options(nomem, nostack, preserves_flags));
}

/// Read a byte from an I/O port.
unsafe fn inb(port: u16) -> u8 {
    let value: u8;
    asm!("in al, dx", in("dx") port, out("al") value, options(nomem, nostack, preserves_flags));
    value
}

/// Small I/O delay.
unsafe fn io_wait() {
    outb(0x80, 0);
}

/// Initialize and remap the 8259 PIC.
/// Remaps IRQ 0-7 to interrupts 32-39, IRQ 8-15 to interrupts 40-47.
pub fn init() {
    unsafe {
        // Save masks
        let mask1 = inb(PIC1_DATA);
        let mask2 = inb(PIC2_DATA);

        // ICW1: Start initialization sequence
        outb(PIC1_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();
        outb(PIC2_CMD, ICW1_INIT | ICW1_ICW4);
        io_wait();

        // ICW2: Set vector offsets
        outb(PIC1_DATA, PIC1_OFFSET);
        io_wait();
        outb(PIC2_DATA, PIC2_OFFSET);
        io_wait();

        // ICW3: Tell PICs about each other
        outb(PIC1_DATA, 4); // Slave PIC at IRQ2
        io_wait();
        outb(PIC2_DATA, 2); // Slave cascade identity
        io_wait();

        // ICW4: 8086 mode
        outb(PIC1_DATA, ICW4_8086);
        io_wait();
        outb(PIC2_DATA, ICW4_8086);
        io_wait();

        // Restore masks (mask all except timer=IRQ0 and keyboard=IRQ1)
        let _ = mask1;
        let _ = mask2;
        outb(PIC1_DATA, 0xF8); // IRQ0 (timer), IRQ1 (keyboard), IRQ2 (cascade to slave) enabled
        outb(PIC2_DATA, 0xEF); // IRQ12 (mouse) enabled on slave PIC
    }
}

/// Send End-of-Interrupt signal to the PIC(s).
pub fn send_eoi(irq: u8) {
    unsafe {
        if irq >= 8 {
            outb(PIC2_CMD, 0x20);
        }
        outb(PIC1_CMD, 0x20);
    }
}

/// Disable the PIC (when switching to APIC later).
#[allow(dead_code)]
pub fn disable() {
    unsafe {
        outb(PIC1_DATA, 0xFF);
        outb(PIC2_DATA, 0xFF);
    }
}
