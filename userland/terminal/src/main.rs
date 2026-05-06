#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec::Vec;
use libhuseyn::{ipc, syscall, SYS_SHM_CREATE, SYS_SHM_MAP};

const WIDTH: usize = 400;
const HEIGHT: usize = 300;
const SHM_VADDR: u64 = 0xC000_0000;
const COMPOSITOR_PID: usize = 5; // init=1, console=2, kbd=3, mouse=4, compositor=5, terminal=6

// Colors
const BG_COLOR: u32 = 0xFF1E1E1E;   // Dark grey
const TEXT_COLOR: u32 = 0xFFD4D4D4; // Light grey

struct Terminal {
    buffer: *mut u32,
    shm_id: u32,
    cursor_x: usize,
    cursor_y: usize,
    #[allow(dead_code)]
    text_buffer: Vec<u8>,
}

impl Terminal {
    fn new() -> Self {
        Self {
            buffer: core::ptr::null_mut(),
            shm_id: 0,
            cursor_x: 5,
            cursor_y: 5, // No title bar in client area anymore
            text_buffer: Vec::new(),
        }
    }

    fn init(&mut self) {
        let size = (WIDTH * HEIGHT * 4) as u64;

        // 1. Create SHM region
        unsafe {
            let res = syscall(SYS_SHM_CREATE, size, 0, 0, 0, 0, 0);
            if res == u64::MAX {
                libhuseyn::print!("Terminal: Failed to create SHM\n");
                libhuseyn::exit(1);
            }
            self.shm_id = res as u32;

            // 2. Map SHM to our address space
            let map_res = syscall(SYS_SHM_MAP, self.shm_id as u64, SHM_VADDR, 0, 0, 0, 0);
            if map_res == u64::MAX {
                libhuseyn::print!("Terminal: Failed to map SHM\n");
                libhuseyn::exit(1);
            }
            self.buffer = SHM_VADDR as *mut u32;
        }

        // 3. Clear window to background color
        self.clear();

        // 4. Title bar is now drawn by the compositor (server-side decoration)

        // 5. Tell compositor about our window
        let mut msg = ipc::Message::empty();
        msg.msg_type = ipc::MSG_WINDOW_CREATE;
        msg.arg1 = WIDTH as u64;
        msg.arg2 = HEIGHT as u64;
        msg.arg3 = self.shm_id as u64;
        while !ipc::send(COMPOSITOR_PID, &msg) {
            libhuseyn::yield_now();
        }

        self.print("HuseynOS GUI Terminal\n");
        self.print("root@huseynos:~$ ");
    }

    fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < WIDTH && y < HEIGHT {
            unsafe {
                *self.buffer.add(y * WIDTH + x) = color;
            }
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for dy in 0..h {
            for dx in 0..w {
                self.set_pixel(x + dx, y + dy, color);
            }
        }
    }

    fn clear(&mut self) {
        self.fill_rect(0, 0, WIDTH, HEIGHT, BG_COLOR);
    }

    fn draw_char(&mut self, ch: u8, x: usize, y: usize, color: u32) {
        let glyph = get_mini_glyph(ch);
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 != 0 {
                    self.set_pixel(x + col, y + row, color);
                }
            }
        }
    }

    #[allow(dead_code)]
    fn draw_string(&mut self, mut x: usize, y: usize, text: &str, color: u32) {
        for ch in text.bytes() {
            self.draw_char(ch, x, y, color);
            x += 7;
        }
    }

    fn print(&mut self, s: &str) {
        for ch in s.bytes() {
            if ch == b'\n' {
                self.cursor_x = 5;
                self.cursor_y += 10;
            } else {
                self.draw_char(ch, self.cursor_x, self.cursor_y, TEXT_COLOR);
                self.cursor_x += 7;
                if self.cursor_x + 7 >= WIDTH {
                    self.cursor_x = 5;
                    self.cursor_y += 10;
                }
            }
        }
        self.update_compositor();
    }

    fn update_compositor(&self) {
        let mut msg = ipc::Message::empty();
        msg.msg_type = ipc::MSG_WINDOW_DAMAGE;
        while !ipc::send(COMPOSITOR_PID, &msg) {
            libhuseyn::yield_now();
        }
    }

    fn handle_key(&mut self, key: u8) {
        if key == b'\n' {
            self.print("\nroot@huseynos:~$ ");
        } else if key == 0x08 {
            // Backspace (simple implementation)
            if self.cursor_x > 5 + 17 * 7 { // don't delete prompt
                self.cursor_x -= 7;
                self.fill_rect(self.cursor_x, self.cursor_y, 7, 8, BG_COLOR);
                self.update_compositor();
            }
        } else if key >= 32 && key <= 126 {
            let mut buf = [0u8; 1];
            buf[0] = key;
            if let Ok(s) = core::str::from_utf8(&buf) {
                self.print(s);
            }
        }
    }
}

// Minimal 5x7 font (same as compositor)
fn get_mini_glyph(ch: u8) -> [u8; 7] {
    match ch {
        b'A' => [0b01110, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        b'B' => [0b11110, 0b10001, 0b11110, 0b10001, 0b10001, 0b10001, 0b11110],
        b'C' => [0b01110, 0b10001, 0b10000, 0b10000, 0b10000, 0b10001, 0b01110],
        b'D' => [0b11110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11110],
        b'E' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b11111],
        b'F' => [0b11111, 0b10000, 0b11110, 0b10000, 0b10000, 0b10000, 0b10000],
        b'G' => [0b01110, 0b10001, 0b10000, 0b10111, 0b10001, 0b10001, 0b01110],
        b'H' => [0b10001, 0b10001, 0b10001, 0b11111, 0b10001, 0b10001, 0b10001],
        b'I' => [0b01110, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'J' => [0b00111, 0b00010, 0b00010, 0b00010, 0b10010, 0b10010, 0b01100],
        b'K' => [0b10001, 0b10010, 0b10100, 0b11000, 0b10100, 0b10010, 0b10001],
        b'L' => [0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b10000, 0b11111],
        b'M' => [0b10001, 0b11011, 0b10101, 0b10101, 0b10001, 0b10001, 0b10001],
        b'N' => [0b10001, 0b11001, 0b10101, 0b10011, 0b10001, 0b10001, 0b10001],
        b'O' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'P' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10000, 0b10000, 0b10000],
        b'Q' => [0b01110, 0b10001, 0b10001, 0b10001, 0b10101, 0b10010, 0b01101],
        b'R' => [0b11110, 0b10001, 0b10001, 0b11110, 0b10100, 0b10010, 0b10001],
        b'S' => [0b01110, 0b10001, 0b10000, 0b01110, 0b00001, 0b10001, 0b01110],
        b'T' => [0b11111, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100, 0b00100],
        b'U' => [0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b01110],
        b'V' => [0b10001, 0b10001, 0b10001, 0b10001, 0b01010, 0b01010, 0b00100],
        b'W' => [0b10001, 0b10001, 0b10001, 0b10101, 0b10101, 0b11011, 0b10001],
        b'X' => [0b10001, 0b10001, 0b01010, 0b00100, 0b01010, 0b10001, 0b10001],
        b'Y' => [0b10001, 0b10001, 0b01010, 0b00100, 0b00100, 0b00100, 0b00100],
        b'Z' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b10000, 0b11111],
        b'a'..=b'z' => get_mini_glyph(ch - 32),
        b'0' => [0b01110, 0b10001, 0b10011, 0b10101, 0b11001, 0b10001, 0b01110],
        b'1' => [0b00100, 0b01100, 0b00100, 0b00100, 0b00100, 0b00100, 0b01110],
        b'2' => [0b01110, 0b10001, 0b00001, 0b00010, 0b00100, 0b01000, 0b11111],
        b'3' => [0b01110, 0b10001, 0b00001, 0b00110, 0b00001, 0b10001, 0b01110],
        b'4' => [0b00010, 0b00110, 0b01010, 0b10010, 0b11111, 0b00010, 0b00010],
        b'5' => [0b11111, 0b10000, 0b11110, 0b00001, 0b00001, 0b10001, 0b01110],
        b'6' => [0b01110, 0b10000, 0b11110, 0b10001, 0b10001, 0b10001, 0b01110],
        b'7' => [0b11111, 0b00001, 0b00010, 0b00100, 0b01000, 0b01000, 0b01000],
        b'8' => [0b01110, 0b10001, 0b10001, 0b01110, 0b10001, 0b10001, 0b01110],
        b'9' => [0b01110, 0b10001, 0b10001, 0b01111, 0b00001, 0b00001, 0b01110],
        b' ' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00000],
        b'.' => [0b00000, 0b00000, 0b00000, 0b00000, 0b00000, 0b00100, 0b00100],
        b':' => [0b00000, 0b00100, 0b00100, 0b00000, 0b00100, 0b00100, 0b00000],
        b'-' => [0b00000, 0b00000, 0b00000, 0b11111, 0b00000, 0b00000, 0b00000],
        b'~' => [0b00000, 0b00000, 0b01000, 0b10101, 0b00010, 0b00000, 0b00000],
        b'$' => [0b00100, 0b01111, 0b10100, 0b01110, 0b00101, 0b11110, 0b00100],
        b'>' => [0b10000, 0b01000, 0b00100, 0b00010, 0b00100, 0b01000, 0b10000],
        b'@' => [0b01110, 0b10001, 0b10101, 0b10111, 0b10000, 0b10000, 0b01110],
        b'!' => [0b00100, 0b00100, 0b00100, 0b00100, 0b00000, 0b00000, 0b00100],
        _    => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
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
                // Graceful shutdown requested by compositor
                libhuseyn::exit(0);
            }
            _ => {}
        }
    }
}
