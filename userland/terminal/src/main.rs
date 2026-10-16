#![no_std]
#![no_main]

extern crate alloc;
use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use libhuseyn::{
    ipc, syscall, open, read, close, listdir, spawn, get_time, shutdown,
    SYS_SHM_CREATE, SYS_SHM_MAP,
};

const WIDTH: usize = 520;
const HEIGHT: usize = 340;
const SHM_VADDR: u64 = 0xC000_0000;
const COMPOSITOR_PID: usize = 5;

const PAD_X: usize = 10;
const PAD_Y: usize = 10;
const CHAR_W: usize = 7;
const LINE_H: usize = 11;

// Cyber-Monolith Terminal Palette
const BG_COLOR: u32     = 0xFF0A0D14; // Deep obsidian slate
const TEXT_COLOR: u32   = 0xFFE2E8F0; // Crisp off-white
const CYAN_COLOR: u32   = 0xFF00E5FF; // Prompt node accent
const AMBER_COLOR: u32  = 0xFFFFB000; // Prompt symbol & highlights
const MUTED_COLOR: u32  = 0xFF64748B; // Telemetry & borders
const CURSOR_COLOR: u32 = 0xFF00E5FF; // Block cursor

const SYS_WRITE_FILE: u64 = 20;
const SYS_UNLINK: u64     = 26;
const SYS_PROCLIST: u64   = 27;
const SYS_SYSINFO: u64    = 28;
const SYS_INSTALL_OS: u64 = 29;

struct Terminal {
    buffer: *mut u32,
    shm_id: u32,
    cursor_x: usize,
    cursor_y: usize,
    cmd_buf: String,
}

impl Terminal {
    fn new() -> Self {
        Self {
            buffer: core::ptr::null_mut(),
            shm_id: 0,
            cursor_x: PAD_X,
            cursor_y: PAD_Y,
            cmd_buf: String::with_capacity(128),
        }
    }

    fn init(&mut self) {
        let size = (WIDTH * HEIGHT * 4) as u64;

        unsafe {
            let res = syscall(SYS_SHM_CREATE, size, 0, 0, 0, 0, 0);
            if res == u64::MAX {
                libhuseyn::exit(1);
            }
            self.shm_id = res as u32;

            let map_res = syscall(SYS_SHM_MAP, self.shm_id as u64, SHM_VADDR, 0, 0, 0, 0);
            if map_res == u64::MAX {
                libhuseyn::exit(1);
            }
            self.buffer = SHM_VADDR as *mut u32;
        }

        self.clear_canvas();

        let mut msg = ipc::Message::empty();
        msg.msg_type = ipc::MSG_WINDOW_CREATE;
        msg.arg1 = WIDTH as u64;
        msg.arg2 = HEIGHT as u64;
        msg.arg3 = self.shm_id as u64;
        while !ipc::send(COMPOSITOR_PID, &msg) {
            libhuseyn::yield_now();
        }

        self.print_banner();
        self.print_prompt();
        self.draw_cursor_block(true);
        self.update_compositor();
    }

    fn print_banner(&mut self) {
        self.print_colored("HUSEYN//OS MONOLITH SHELL v1.0\n", CYAN_COLOR);
        self.print_colored("Type 'help' | 'install' 'disks' 'usb' 'acpi' 'lspci' 'ps' 'mem'\n\n", MUTED_COLOR);
    }

    fn print_prompt(&mut self) {
        self.print_colored("huseyn@monolith", CYAN_COLOR);
        self.print_colored(" ~ > ", AMBER_COLOR);
    }

    #[inline]
    fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < WIDTH && y < HEIGHT {
            unsafe {
                *self.buffer.add(y * WIDTH + x) = color;
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for dy in 0..h {
            let py = y + dy;
            if py >= HEIGHT { break; }
            for dx in 0..w {
                let px = x + dx;
                if px >= WIDTH { break; }
                unsafe {
                    *self.buffer.add(py * WIDTH + px) = color;
                }
            }
        }
    }

    fn clear_canvas(&mut self) {
        self.fill_rect(0, 0, WIDTH, HEIGHT, BG_COLOR);
        self.fill_rect(0, 0, WIDTH, 1, 0xFF162030);
        self.cursor_x = PAD_X;
        self.cursor_y = PAD_Y;
    }

    fn scroll_up(&mut self) {
        let scroll_px = LINE_H;
        unsafe {
            let src = self.buffer.add((PAD_Y + scroll_px) * WIDTH);
            let dst = self.buffer.add(PAD_Y * WIDTH);
            let count = (HEIGHT - PAD_Y - scroll_px) * WIDTH;
            core::ptr::copy(src, dst, count);
        }
        self.fill_rect(0, HEIGHT - scroll_px, WIDTH, scroll_px, BG_COLOR);
        self.cursor_y = self.cursor_y.saturating_sub(scroll_px);
    }

    fn newline(&mut self) {
        self.cursor_x = PAD_X;
        self.cursor_y += LINE_H;
        if self.cursor_y + LINE_H >= HEIGHT - 4 {
            self.scroll_up();
        }
    }

    fn draw_char(&mut self, ch: u8, x: usize, y: usize, color: u32) {
        let glyph = get_font_glyph(ch);
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..6 {
                if (bits >> (5 - col)) & 1 != 0 {
                    self.set_pixel(x + col, y + row, color);
                }
            }
        }
    }

    fn draw_cursor_block(&mut self, visible: bool) {
        let color = if visible { CURSOR_COLOR } else { BG_COLOR };
        self.fill_rect(self.cursor_x, self.cursor_y + 7, 5, 2, color);
    }

    fn print_colored(&mut self, s: &str, color: u32) {
        for ch in s.bytes() {
            if ch == b'\n' {
                self.newline();
            } else {
                self.draw_char(ch, self.cursor_x, self.cursor_y, color);
                self.cursor_x += CHAR_W;
                if self.cursor_x + CHAR_W >= WIDTH - PAD_X {
                    self.newline();
                }
            }
        }
    }

    fn update_compositor(&self) {
        let mut msg = ipc::Message::empty();
        msg.msg_type = ipc::MSG_WINDOW_DAMAGE;
        while !ipc::send(COMPOSITOR_PID, &msg) {
            libhuseyn::yield_now();
        }
    }

    fn query_sysinfo(&mut self, mode: u64, color: u32) {
        let mut buf = [0u8; 1024];
        let n = unsafe {
            syscall(SYS_SYSINFO, buf.as_mut_ptr() as u64, buf.len() as u64, mode, 0, 0, 0) as usize
        };
        if n > 0 && n <= buf.len() {
            if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                self.print_colored(s, color);
            }
        }
    }

    fn execute_command(&mut self) {
        let line = self.cmd_buf.trim();
        if line.is_empty() {
            return;
        }

        let mut parts = line.split_whitespace();
        let cmd = parts.next().unwrap_or("");
        let args: Vec<&str> = parts.collect();

        match cmd {
            "help" => {
                self.print_colored("Available Monolith Commands:\n", AMBER_COLOR);
                self.print_colored("  install             Format GPT & install OS to disk\n", CYAN_COLOR);
                self.print_colored("  disks               Show GPT partition & disk status\n", TEXT_COLOR);
                self.print_colored("  usb                 Show xHCI USB 3.0 & HID ports\n", TEXT_COLOR);
                self.print_colored("  acpi                Show ACPI MADT, APIC & IOAPIC\n", TEXT_COLOR);
                self.print_colored("  lspci               Enumerate PCI hardware bus\n", TEXT_COLOR);
                self.print_colored("  ps                  List active kernel tasks\n", TEXT_COLOR);
                self.print_colored("  mem                 Show RAM & PAT WC telemetry\n", TEXT_COLOR);
                self.print_colored("  ls / cat / rm       Inspect or delete files\n", TEXT_COLOR);
                self.print_colored("  write <file> <txt>  Write persistent file to disk\n", TEXT_COLOR);
                self.print_colored("  run <file.elf>      Spawn userland ELF binary\n", TEXT_COLOR);
                self.print_colored("  clear / off         Clear screen or ACPI shutdown\n", TEXT_COLOR);
            }
            "clear" => {
                self.clear_canvas();
            }
            "install" => {
                self.print_colored("Installing HuseynOS to physical disk (GPT + FAT)...\n", AMBER_COLOR);
                self.update_compositor();
                let mut buf = [0u8; 1024];
                let n = unsafe {
                    syscall(SYS_INSTALL_OS, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0) as usize
                };
                if n > 0 && n <= buf.len() {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        self.print_colored(s, CYAN_COLOR);
                    }
                }
            }
            "disks" | "disk" => {
                self.query_sysinfo(4, TEXT_COLOR);
            }
            "usb" | "xhci" => {
                self.query_sysinfo(3, TEXT_COLOR);
            }
            "acpi" | "apic" => {
                self.query_sysinfo(2, TEXT_COLOR);
            }
            "lspci" | "pci" => {
                self.query_sysinfo(1, TEXT_COLOR);
            }
            "mem" => {
                self.query_sysinfo(0, TEXT_COLOR);
            }
            "ls" => {
                let mut buf = [0u8; 2048];
                let n = listdir(&mut buf);
                if n > 0 {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        self.print_colored("NAME            SIZE (BYTES)\n", MUTED_COLOR);
                        for entry in s.lines() {
                            let mut sp = entry.rsplitn(2, ' ');
                            if let (Some(sz), Some(name)) = (sp.next(), sp.next()) {
                                let mut row = String::new();
                                let _ = writeln!(row, "{:<15} {}", name, sz);
                                self.print_colored(&row, TEXT_COLOR);
                            }
                        }
                    }
                } else {
                    self.print_colored("(empty volume)\n", MUTED_COLOR);
                }
            }
            "cat" => {
                if args.is_empty() {
                    self.print_colored("Usage: cat <filename>\n", AMBER_COLOR);
                } else {
                    match open(args[0], 0) {
                        Ok(fd) => {
                            let mut buf = [0u8; 1024];
                            let n = read(fd, &mut buf);
                            close(fd);
                            if n > 0 {
                                if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                                    self.print_colored(s, TEXT_COLOR);
                                    if !s.ends_with('\n') {
                                        self.newline();
                                    }
                                } else {
                                    self.print_colored("[binary file content]\n", MUTED_COLOR);
                                }
                            } else {
                                self.print_colored("(empty file)\n", MUTED_COLOR);
                            }
                        }
                        Err(_) => {
                            self.print_colored("Error: file not found\n", AMBER_COLOR);
                        }
                    }
                }
            }
            "write" => {
                if args.len() < 2 {
                    self.print_colored("Usage: write <filename> <text...>\n", AMBER_COLOR);
                } else {
                    let filename = args[0];
                    let mut content = String::new();
                    for (i, word) in args[1..].iter().enumerate() {
                        if i > 0 { content.push(' '); }
                        content.push_str(word);
                    }
                    content.push('\n');

                    match open(filename, 1) {
                        Ok(fd) => {
                            let bytes = content.as_bytes();
                            let written = unsafe {
                                syscall(SYS_WRITE_FILE, fd as u64, bytes.as_ptr() as u64, bytes.len() as u64, 0, 0, 0)
                            };
                            close(fd);
                            let mut msg = String::new();
                            let _ = writeln!(msg, "[OK] Wrote {} bytes to '{}' (synced to disk)", written, filename);
                            self.print_colored(&msg, CYAN_COLOR);
                        }
                        Err(_) => {
                            self.print_colored("Error: failed to open file for writing\n", AMBER_COLOR);
                        }
                    }
                }
            }
            "rm" => {
                if args.is_empty() {
                    self.print_colored("Usage: rm <filename>\n", AMBER_COLOR);
                } else {
                    let fname = args[0];
                    let res = unsafe {
                        syscall(SYS_UNLINK, fname.as_ptr() as u64, fname.len() as u64, 0, 0, 0, 0)
                    };
                    if res == 0 {
                        let mut msg = String::new();
                        let _ = writeln!(msg, "[OK] Removed '{}'", fname);
                        self.print_colored(&msg, CYAN_COLOR);
                    } else {
                        self.print_colored("Error: file not found\n", AMBER_COLOR);
                    }
                }
            }
            "ps" => {
                let mut buf = [0u8; 1024];
                let n = unsafe {
                    syscall(SYS_PROCLIST, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0) as usize
                };
                if n > 0 && n <= buf.len() {
                    if let Ok(s) = core::str::from_utf8(&buf[..n]) {
                        self.print_colored(s, TEXT_COLOR);
                    }
                }
            }
            "run" => {
                if args.is_empty() {
                    self.print_colored("Usage: run <program.elf>\n", AMBER_COLOR);
                } else {
                    match spawn(args[0]) {
                        Ok(pid) => {
                            let mut msg = String::new();
                            let _ = writeln!(msg, "[OK] Spawned '{}' as PID {}", args[0], pid);
                            self.print_colored(&msg, CYAN_COLOR);
                        }
                        Err(_) => {
                            self.print_colored("Error: failed to spawn ELF binary\n", AMBER_COLOR);
                        }
                    }
                }
            }
            "sys" | "uname" => {
                let dt = get_time();
                let pid = libhuseyn::getpid();
                let mut info = String::new();
                let _ = writeln!(info, "OS:       HuseynOS v1.0 (Monolith Microkernel)");
                let _ = writeln!(info, "Arch:     x86_64 (#![no_std] Rust, Bare-Metal Ready)");
                let _ = writeln!(info, "Term PID: {}", pid);
                let _ = writeln!(
                    info,
                    "CMOS RTC: {:04}-{:02}-{:02} {:02}:{:02}:{:02} UTC",
                    dt.year, dt.month, dt.day, dt.hour, dt.minute, dt.second
                );
                self.print_colored(&info, TEXT_COLOR);
            }
            "off" | "shutdown" => {
                self.print_colored("Initiating ACPI poweroff...\n", AMBER_COLOR);
                self.update_compositor();
                shutdown();
            }
            _ => {
                let mut err = String::new();
                let _ = writeln!(err, "Unknown command: '{}'. Type 'help'.", cmd);
                self.print_colored(&err, AMBER_COLOR);
            }
        }
    }

    fn handle_key(&mut self, key: u8) {
        self.draw_cursor_block(false);

        if key == b'\n' {
            self.newline();
            let current_cmd = self.cmd_buf.clone();
            self.cmd_buf.clear();
            if !current_cmd.trim().is_empty() {
                self.cmd_buf = current_cmd;
                self.execute_command();
                self.cmd_buf.clear();
            }
            self.print_prompt();
        } else if key == 0x08 {
            if !self.cmd_buf.is_empty() {
                self.cmd_buf.pop();
                if self.cursor_x >= PAD_X + CHAR_W {
                    self.cursor_x -= CHAR_W;
                    self.fill_rect(self.cursor_x, self.cursor_y, CHAR_W, LINE_H, BG_COLOR);
                }
            }
        } else if (32..=126).contains(&key) && self.cmd_buf.len() < 60 {
            self.cmd_buf.push(key as char);
            let buf = [key];
            if let Ok(s) = core::str::from_utf8(&buf) {
                self.print_colored(s, TEXT_COLOR);
            }
        }

        self.draw_cursor_block(true);
        self.update_compositor();
    }
}

fn get_font_glyph(ch: u8) -> [u8; 8] {
    match ch {
        b'A' => [0b011100, 0b100010, 0b100010, 0b111110, 0b100010, 0b100010, 0b100010, 0],
        b'B' => [0b111100, 0b100010, 0b111100, 0b100010, 0b100010, 0b100010, 0b111100, 0],
        b'C' => [0b011100, 0b100010, 0b100000, 0b100000, 0b100000, 0b100010, 0b011100, 0],
        b'D' => [0b111100, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b111100, 0],
        b'E' => [0b111110, 0b100000, 0b111100, 0b100000, 0b100000, 0b100000, 0b111110, 0],
        b'F' => [0b111110, 0b100000, 0b111100, 0b100000, 0b100000, 0b100000, 0b100000, 0],
        b'G' => [0b011100, 0b100010, 0b100000, 0b101110, 0b100010, 0b100010, 0b011100, 0],
        b'H' => [0b100010, 0b100010, 0b100010, 0b111110, 0b100010, 0b100010, 0b100010, 0],
        b'I' => [0b011100, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0],
        b'J' => [0b001110, 0b000100, 0b000100, 0b000100, 0b100100, 0b100100, 0b011000, 0],
        b'K' => [0b100010, 0b100100, 0b101000, 0b110000, 0b101000, 0b100100, 0b100010, 0],
        b'L' => [0b100000, 0b100000, 0b100000, 0b100000, 0b100000, 0b100000, 0b111110, 0],
        b'M' => [0b100010, 0b110110, 0b101010, 0b101010, 0b100010, 0b100010, 0b100010, 0],
        b'N' => [0b100010, 0b110010, 0b101010, 0b100110, 0b100010, 0b100010, 0b100010, 0],
        b'O' => [0b011100, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0],
        b'P' => [0b111100, 0b100010, 0b100010, 0b111100, 0b100000, 0b100000, 0b100000, 0],
        b'Q' => [0b011100, 0b100010, 0b100010, 0b100010, 0b101010, 0b100100, 0b011010, 0],
        b'R' => [0b111100, 0b100010, 0b100010, 0b111100, 0b101000, 0b100100, 0b100010, 0],
        b'S' => [0b011100, 0b100010, 0b100000, 0b011100, 0b000010, 0b100010, 0b011100, 0],
        b'T' => [0b111110, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0],
        b'U' => [0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b011100, 0],
        b'V' => [0b100010, 0b100010, 0b100010, 0b100010, 0b010100, 0b010100, 0b001000, 0],
        b'W' => [0b100010, 0b100010, 0b100010, 0b101010, 0b101010, 0b110110, 0b100010, 0],
        b'X' => [0b100010, 0b100010, 0b010100, 0b001000, 0b010100, 0b100010, 0b100010, 0],
        b'Y' => [0b100010, 0b100010, 0b010100, 0b001000, 0b001000, 0b001000, 0b001000, 0],
        b'Z' => [0b111110, 0b000010, 0b000100, 0b001000, 0b010000, 0b100000, 0b111110, 0],

        b'a' => [0b000000, 0b000000, 0b011100, 0b000010, 0b011110, 0b100010, 0b011110, 0],
        b'b' => [0b100000, 0b100000, 0b111100, 0b100010, 0b100010, 0b100010, 0b111100, 0],
        b'c' => [0b000000, 0b000000, 0b011100, 0b100010, 0b100000, 0b100010, 0b011100, 0],
        b'd' => [0b000010, 0b000010, 0b011110, 0b100010, 0b100010, 0b100010, 0b011110, 0],
        b'e' => [0b000000, 0b000000, 0b011100, 0b100010, 0b111110, 0b100000, 0b011100, 0],
        b'f' => [0b001100, 0b010010, 0b010000, 0b111000, 0b010000, 0b010000, 0b010000, 0],
        b'g' => [0b000000, 0b000000, 0b011110, 0b100010, 0b100010, 0b011110, 0b000010, 0b011100],
        b'h' => [0b100000, 0b100000, 0b111100, 0b100010, 0b100010, 0b100010, 0b100010, 0],
        b'i' => [0b001000, 0b000000, 0b011000, 0b001000, 0b001000, 0b001000, 0b011100, 0],
        b'j' => [0b000100, 0b000000, 0b001100, 0b000100, 0b000100, 0b100100, 0b011000, 0],
        b'k' => [0b100000, 0b100000, 0b100100, 0b101000, 0b110000, 0b101000, 0b100100, 0],
        b'l' => [0b011000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0],
        b'm' => [0b000000, 0b000000, 0b110100, 0b101010, 0b101010, 0b100010, 0b100010, 0],
        b'n' => [0b000000, 0b000000, 0b111100, 0b100010, 0b100010, 0b100010, 0b100010, 0],
        b'o' => [0b000000, 0b000000, 0b011100, 0b100010, 0b100010, 0b100010, 0b011100, 0],
        b'p' => [0b000000, 0b000000, 0b111100, 0b100010, 0b100010, 0b111100, 0b100000, 0b100000],
        b'q' => [0b000000, 0b000000, 0b011110, 0b100010, 0b100010, 0b011110, 0b000010, 0b000010],
        b'r' => [0b000000, 0b000000, 0b101100, 0b110010, 0b100000, 0b100000, 0b100000, 0],
        b's' => [0b000000, 0b000000, 0b011110, 0b100000, 0b011100, 0b000010, 0b111100, 0],
        b't' => [0b010000, 0b010000, 0b111100, 0b010000, 0b010000, 0b010010, 0b001100, 0],
        b'u' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b100010, 0b011110, 0],
        b'v' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b010100, 0b001000, 0],
        b'w' => [0b000000, 0b000000, 0b100010, 0b100010, 0b101010, 0b110110, 0b100010, 0],
        b'x' => [0b000000, 0b000000, 0b100010, 0b010100, 0b001000, 0b010100, 0b100010, 0],
        b'y' => [0b000000, 0b000000, 0b100010, 0b100010, 0b100010, 0b011110, 0b000010, 0b011100],
        b'z' => [0b000000, 0b000000, 0b111110, 0b000100, 0b001000, 0b010000, 0b111110, 0],

        b'0' => [0b011100, 0b100010, 0b100110, 0b101010, 0b110010, 0b100010, 0b011100, 0],
        b'1' => [0b001000, 0b011000, 0b001000, 0b001000, 0b001000, 0b001000, 0b011100, 0],
        b'2' => [0b011100, 0b100010, 0b000010, 0b000100, 0b001000, 0b010000, 0b111110, 0],
        b'3' => [0b011100, 0b100010, 0b000010, 0b001100, 0b000010, 0b100010, 0b011100, 0],
        b'4' => [0b000100, 0b001100, 0b010100, 0b100100, 0b111110, 0b000100, 0b000100, 0],
        b'5' => [0b111110, 0b100000, 0b111100, 0b000010, 0b000010, 0b100010, 0b011100, 0],
        b'6' => [0b011100, 0b100000, 0b111100, 0b100010, 0b100010, 0b100010, 0b011100, 0],
        b'7' => [0b111110, 0b000010, 0b000100, 0b001000, 0b010000, 0b010000, 0b010000, 0],
        b'8' => [0b011100, 0b100010, 0b100010, 0b011100, 0b100010, 0b100010, 0b011100, 0],
        b'9' => [0b011100, 0b100010, 0b100010, 0b011110, 0b000010, 0b000010, 0b011100, 0],

        b' ' => [0; 8],
        b'.' => [0, 0, 0, 0, 0, 0b001000, 0b001000, 0],
        b',' => [0, 0, 0, 0, 0, 0b001000, 0b001000, 0b010000],
        b':' => [0, 0b001000, 0b001000, 0, 0b001000, 0b001000, 0, 0],
        b';' => [0, 0b001000, 0b001000, 0, 0b001000, 0b001000, 0b010000, 0],
        b'-' => [0, 0, 0, 0b111110, 0, 0, 0, 0],
        b'_' => [0, 0, 0, 0, 0, 0, 0b111110, 0],
        b'+' => [0, 0b001000, 0b001000, 0b111110, 0b001000, 0b001000, 0, 0],
        b'=' => [0, 0, 0b111110, 0, 0b111110, 0, 0, 0],
        b'/' => [0b000010, 0b000100, 0b000100, 0b001000, 0b010000, 0b010000, 0b100000, 0],
        b'\\' => [0b100000, 0b010000, 0b010000, 0b001000, 0b000100, 0b000100, 0b000010, 0],
        b'|' => [0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0],
        b'[' => [0b011100, 0b010000, 0b010000, 0b010000, 0b010000, 0b010000, 0b011100, 0],
        b']' => [0b011100, 0b000100, 0b000100, 0b000100, 0b000100, 0b000100, 0b011100, 0],
        b'(' => [0b000100, 0b001000, 0b010000, 0b010000, 0b010000, 0b001000, 0b000100, 0],
        b')' => [0b010000, 0b001000, 0b000100, 0b000100, 0b000100, 0b001000, 0b010000, 0],
        b'<' => [0b000100, 0b001000, 0b010000, 0b100000, 0b010000, 0b001000, 0b000100, 0],
        b'>' => [0b100000, 0b010000, 0b001000, 0b000100, 0b001000, 0b010000, 0b100000, 0],
        b'~' => [0, 0, 0b010000, 0b101010, 0b000100, 0, 0, 0],
        b'$' => [0b001000, 0b011110, 0b101000, 0b011100, 0b001010, 0b111100, 0b001000, 0],
        b'@' => [0b011100, 0b100010, 0b101110, 0b101010, 0b101110, 0b100000, 0b011100, 0],
        b'!' => [0b001000, 0b001000, 0b001000, 0b001000, 0b001000, 0, 0b001000, 0],
        b'?' => [0b011100, 0b100010, 0b000100, 0b001000, 0b001000, 0, 0b001000, 0],
        b'*' => [0, 0b101010, 0b011100, 0b111110, 0b011100, 0b101010, 0, 0],
        b'#' => [0b010100, 0b111110, 0b010100, 0b010100, 0b111110, 0b010100, 0, 0],
        b'"' => [0b010100, 0b010100, 0, 0, 0, 0, 0, 0],
        b'\'' => [0b001000, 0b001000, 0, 0, 0, 0, 0, 0],
        _    => [0b111110, 0b100010, 0b100010, 0b100010, 0b100010, 0b100010, 0b111110, 0],
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut term = Terminal::new();
    term.init();

    loop {
        let msg = ipc::receive();
        match msg.msg_type {
            ipc::MSG_KEY_PRESSED => {
                term.handle_key(msg.arg1 as u8);
            }
            ipc::MSG_QUIT => {
                libhuseyn::exit(0);
            }
            _ => {}
        }
    }
}
