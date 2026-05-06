#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use libhuseyn::{print, println};
use libhuseyn::{ipc, spawn, waitpid, open, read, close, listdir};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    println!("[Shell] Started in Userland!");

    let mut input_buffer = String::with_capacity(128);

    loop {
        print!("root@huseynos:~$ ");
        input_buffer.clear();

        // Reading line
        loop {
            // libhuseyn receive is blocking right now. We need to be careful if it blocks.
            // Let's just use it, the keyboard driver sends us characters.
            let msg = ipc::receive();
            if msg.msg_type == 0x20 { // MSG_KEY_PRESSED
                let c = msg.arg1 as u8;
                match c {
                        b'\n' => {
                            println!();
                            break;
                        }
                        0x08 => { // Backspace
                            if !input_buffer.is_empty() {
                                input_buffer.pop();
                                print!("{}", 0x08 as char);
                            }
                        }
                        _ => {
                            if input_buffer.len() < input_buffer.capacity() {
                                let ch = c as char;
                                input_buffer.push(ch);
                                print!("{}", ch);
                            }
                        }
                    }
            } else if msg.msg_type == 0xAA { // MSG_PING
                println!("\n[IPC] Shell received PING from PID {}: arg1={:#X}", msg.sender, msg.arg1);
                let mut resp = ipc::Message::empty();
                resp.msg_type = 0xBB;
                ipc::send(msg.sender, &resp);
                print!("root@huseynos:~$ {}", input_buffer);
            } else {
                println!("\n[IPC] Unexpected message from {}: {:#X}", msg.sender, msg.msg_type);
                print!("root@huseynos:~$ {}", input_buffer);
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
            println!("  help        - Show this message");
            println!("  clear       - Clear the screen");
            println!("  ls          - List files in root directory");
            println!("  cat <file>  - Print file contents");
            println!("  run <file>  - Execute an ELF program");
        }
        "ls" => {
            let mut buf = [0u8; 2048];
            let n = listdir(&mut buf);
            if n > 0 {
                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                    // Format: "FILENAME SIZE\n" per line
                    for line in s.lines() {
                        let mut parts = line.rsplitn(2, ' ');
                        if let (Some(size_str), Some(name)) = (parts.next(), parts.next()) {
                            // Right-align file size
                            println!("  {:12}  {} bytes", name, size_str);
                        }
                    }
                }
            } else {
                println!("(empty directory)");
            }
        }
        "cat" => {
            if args.is_empty() {
                println!("Usage: cat <filename>");
            } else {
                let filename = args[0];
                match open(filename, 0) {
                    Ok(fd) => {
                        let mut buf = [0u8; 4096];
                        loop {
                            let n = read(fd, &mut buf);
                            if n == 0 { break; }
                            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                print!("{}", s);
                            }
                        }
                        close(fd);
                    }
                    Err(_) => println!("cat: {}: No such file", filename),
                }
            }
        }
        "run" => {
            if args.is_empty() {
                println!("Usage: run <filename>");
            } else {
                let filename = args[0];
                match spawn(filename) {
                    Ok(pid) => {
                        println!("[OK] Spawned {} (PID {})", filename, pid);
                        match waitpid(pid) {
                            Ok(code) => println!("[Shell] Process {} exited with code {}", pid, code),
                            Err(_) => println!("[Shell] Error waiting for PID {}", pid),
                        }
                    }
                    Err(_) => println!("[ERROR] Failed to spawn '{}'", filename),
                }
            }
        }
        "clear" => {
            print!("\x1B[2J\x1B[1;1H");
        }
        _ => {
            println!("Command not found: {}", cmd);
        }
    }
}

