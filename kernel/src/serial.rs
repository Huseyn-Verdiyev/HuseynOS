use spin::Mutex;
use uart_16550::SerialPort;

/// Global serial port, protected by a spinlock.
pub static SERIAL1: Mutex<Option<SerialPort>> = Mutex::new(None);

/// COM1 I/O port address.
const COM1_PORT: u16 = 0x3F8;

/// Initialize the serial port (COM1).
pub fn init() {
    let mut serial_port = unsafe { SerialPort::new(COM1_PORT) };
    serial_port.init();
    *SERIAL1.lock() = Some(serial_port);
}

/// Write a string to the serial port.
pub fn write_str(s: &str) {
    if let Some(ref mut serial) = *SERIAL1.lock() {
        for byte in s.bytes() {
            serial.send(byte);
        }
    }
}

/// Write a formatted string to the serial port.
pub fn write_fmt(args: core::fmt::Arguments) {
    use core::fmt::Write;

    if let Some(ref mut serial) = *SERIAL1.lock() {
        serial.write_fmt(args).ok();
    }
}

/// Print to the serial console.
#[macro_export]
macro_rules! serial_print {
    ($($arg:tt)*) => {
        $crate::serial::write_fmt(format_args!($($arg)*))
    };
}

/// Print to the serial console, with a newline.
#[macro_export]
macro_rules! serial_println {
    () => ($crate::serial_print!("\n"));
    ($($arg:tt)*) => ({
        $crate::serial::write_fmt(format_args!($($arg)*));
        $crate::serial_print!("\n");
    });
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::serial_print!($($arg)*));
}

#[macro_export]
macro_rules! println {
    () => ($crate::serial_println!());
    ($($arg:tt)*) => ($crate::serial_println!($($arg)*));
}
