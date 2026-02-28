use alloc::string::String;
use alloc::vec::Vec;
use crate::{print, println, keyboard, scheduler, console};

/// The main shell task loop.
pub fn shell_task() {
    println!("[Shell] Started!");
    
    let mut input_buffer = String::with_capacity(128);

    loop {
        print!("root@huseynos:~$ ");
        input_buffer.clear();

        // Reading line
        loop {
            if let Some(c) = keyboard::get_char() {
                match c {
                    b'\n' => {
                        println!();
                        break;
                    }
                    0x08 => { // Backspace
                        if !input_buffer.is_empty() {
                            input_buffer.pop();
                            // Echo backspace to console
                            console::CONSOLE.lock().write_char(0x08);
                        }
                    }
                    _ => {
                        if input_buffer.len() < input_buffer.capacity() {
                            let ch = c as char;
                            input_buffer.push(ch);
                            // Echo character
                            console::CONSOLE.lock().write_char(c);
                        }
                    }
                }
            } else {
                scheduler::yield_now();
            }
        }

        // Process command
        let cmd = input_buffer.trim();
        if cmd.is_empty() {
            continue;
        }

        execute_command(cmd);
    }
}

fn execute_command(cmd_line: &str) {
    let mut parts = cmd_line.split_whitespace();
    let cmd = parts.next().unwrap_or("");
    let args: Vec<&str> = parts.collect();

    match cmd {
        "help" => {
            println!("HuseynOS Shell Commands:");
            println!("  help  - Show this message");
            println!("  clear - Clear the screen");
            println!("  info  - Show system information");
            println!("  date  - Show current date/time (RTC)");
            println!("  ls    - List files on disk");
            println!("  cat   - Read file contents");
        }
        "clear" => {
            console::CONSOLE.lock().clear();
        }
        "info" => {
            println!("HuseynOS v0.5.0 (x86_64)");
            println!("Memory: Frame Allocator & Paging Active");
            println!("Multitasking: Preemptive (PIT 1000Hz)");
            println!("Filesystem: FAT12 (Ramdisk)");
        }
        "date" => {
            let dt = crate::rtc::read_datetime();
            println!(
                "Current time (RTC): {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
            );
        }
        "ls" => {
            let files = crate::fat32::list_files();
            if files.is_empty() {
                println!("(empty disk)");
            } else {
                println!("  {:<16} {:>8}", "NAME", "SIZE");
                println!("  {}", "-".repeat(26));
                for (name, size) in &files {
                    println!("  {:<16} {:>6} B", name, size);
                }
                println!("  {} file(s)", files.len());
            }
        }
        "cat" => {
            if args.is_empty() {
                println!("Usage: cat <filename>");
            } else {
                let filename = args[0];
                match crate::fat32::read_file(filename) {
                    Some(data) => {
                        if let Ok(text) = core::str::from_utf8(&data) {
                            print!("{}", text);
                        } else {
                            println!("<binary data, {} bytes>", data.len());
                        }
                    }
                    None => {
                        println!("File not found: {}", filename);
                    }
                }
            }
        }
        _ => {
            println!("Command not found: {}", cmd);
        }
    }
}
