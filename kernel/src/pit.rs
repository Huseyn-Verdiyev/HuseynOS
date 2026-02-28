use spin::Mutex;
use core::arch::asm;
use crate::serial_println;

const PIT_CMD_PORT: u16 = 0x43;
const PIT_DATA_PORT_0: u16 = 0x40;

/// Base frequency of the PIT in Hz
const PIT_BASE_FREQUENCY: u32 = 1193182;

/// A simple global lock so we don't re-initialize concurrently
static PIT_MUTEX: Mutex<()> = Mutex::new(());

/// Write to the PIT command and data ports
fn write_pit(freq_hz: u32) {
    let divisor = PIT_BASE_FREQUENCY / freq_hz;
    // Command byte: 0x36
    // 00 = Channel 0
    // 11 = Access mode: lobyte/hibyte
    // 011 = Operating mode 3 (Square wave generator)
    // 0 = 16-bit binary
    let cmd: u8 = 0x36;
    let low = (divisor & 0xFF) as u8;
    let high = ((divisor >> 8) & 0xFF) as u8;

    unsafe {
        // Send command
        asm!("out dx, al", in("dx") PIT_CMD_PORT, in("al") cmd, options(nomem, nostack, preserves_flags));
        // Send low byte
        asm!("out dx, al", in("dx") PIT_DATA_PORT_0, in("al") low, options(nomem, nostack, preserves_flags));
        // Send high byte
        asm!("out dx, al", in("dx") PIT_DATA_PORT_0, in("al") high, options(nomem, nostack, preserves_flags));
    }
}

/// Initialize the PIT to a specific frequency (in Hz).
pub fn init(frequency: u32) {
    let _lock = PIT_MUTEX.lock();
    write_pit(frequency);
    serial_println!("[OK] PIT initialized to {} Hz", frequency);
}
