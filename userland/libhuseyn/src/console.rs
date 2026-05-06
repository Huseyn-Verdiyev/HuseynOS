use core::fmt;
use crate::ipc;

pub const CONSOLE_PID: usize = 2;

pub struct ConsoleWriter;

impl fmt::Write for ConsoleWriter {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        for b in s.bytes() {
            let mut msg = ipc::Message::empty();
            msg.msg_type = ipc::MSG_PRINT_CHAR;
            msg.arg1 = b as u64;
            // Send to console driver (PID 1)
            while !ipc::send(CONSOLE_PID, &msg) {
                crate::yield_now();
            }
        }
        Ok(())
    }
}

pub fn print_args(args: fmt::Arguments) {
    use core::fmt::Write;
    let mut writer = ConsoleWriter;
    writer.write_fmt(args).unwrap();
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => ($crate::console::print_args(format_args!($($arg)*)));
}

#[macro_export]
macro_rules! println {
    () => ($crate::print!("\n"));
    ($($arg:tt)*) => ($crate::print!("{}\n", format_args!($($arg)*)));
}
