#![no_std]
#![no_main]

use libhuseyn::{exit, get_fb_info, map_physical};
use libhuseyn::ipc::{receive, MSG_PRINT_CHAR};

mod vga_font;

const COLOR_BLACK: u32 = 0x000000;
const COLOR_WHITE: u32 = 0xFFFFFF;

const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;
const TAB_SIZE: usize = 4;

pub struct Console {
    fb_ptr: *mut u8,
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    x_pos: usize,
    y_pos: usize,
    fg_color: u32,
    bg_color: u32,
}

impl Console {
    fn new() -> Self {
        Self {
            fb_ptr: core::ptr::null_mut(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
            x_pos: 0,
            y_pos: 0,
            fg_color: COLOR_WHITE,
            bg_color: COLOR_BLACK,
        }
    }

    fn init(&mut self) {
        let (paddr, w, h, p, bpp) = get_fb_info();
        self.width = w as usize;
        self.height = h as usize;
        self.pitch = p as usize;
        self.bpp = bpp as usize;
        
        // Map physical framebuffer to a fixed virtual address (e.g., 2GB mark)
        let fb_virt = 0x8000_0000_u64;
        let fb_size = (self.pitch * self.height) as u64;
        
        if map_physical(fb_virt, paddr, fb_size).is_err() {
            exit(1);
        }
        
        self.fb_ptr = fb_virt as *mut u8;
        self.clear();
    }

    fn clear(&mut self) {
        for y in 0..self.height {
            for x in 0..self.width {
                self.plot_pixel(x, y, self.bg_color);
            }
        }
        self.x_pos = 0;
        self.y_pos = 0;
    }

    fn plot_pixel(&self, x: usize, y: usize, color: u32) {
        if x >= self.width || y >= self.height { return; }
        let offset = y * self.pitch + x * self.bpp;
        unsafe {
            let pixel_ptr = self.fb_ptr.add(offset) as *mut u32;
            core::ptr::write_volatile(pixel_ptr, color);
        }
    }

    fn draw_char(&mut self, c: u8, x: usize, y: usize) {
        let glyph = vga_font::get_char_data(c);
        for (row_idx, &row_mask) in glyph.iter().enumerate() {
            for col_idx in 0..FONT_WIDTH {
                if (row_mask >> (FONT_WIDTH - 1 - col_idx)) & 1 != 0 {
                    self.plot_pixel(x + col_idx, y + row_idx, self.fg_color);
                } else {
                    self.plot_pixel(x + col_idx, y + row_idx, self.bg_color);
                }
            }
        }
    }

    fn scroll(&mut self) {
        let row_bytes = self.pitch * FONT_HEIGHT;
        let total_bytes = self.height * self.pitch;
        unsafe {
            let src = self.fb_ptr.add(row_bytes);
            core::ptr::copy(src, self.fb_ptr, total_bytes - row_bytes);
            core::ptr::write_bytes(self.fb_ptr.add(total_bytes - row_bytes), 0, row_bytes);
        }
        self.y_pos -= FONT_HEIGHT;
    }

    fn write_char(&mut self, c: u8) {
        match c {
            b'\n' => {
                self.x_pos = 0;
                self.y_pos += FONT_HEIGHT;
            }
            0x08 => { // Backspace
                if self.x_pos >= FONT_WIDTH {
                    self.x_pos -= FONT_WIDTH;
                } else if self.y_pos >= FONT_HEIGHT {
                    self.y_pos -= FONT_HEIGHT;
                    self.x_pos = self.width - FONT_WIDTH;
                }
                self.draw_char(b' ', self.x_pos, self.y_pos);
            }
            b'\t' => {
                self.x_pos += FONT_WIDTH * TAB_SIZE;
            }
            _ => {
                if self.x_pos + FONT_WIDTH > self.width {
                    self.x_pos = 0;
                    self.y_pos += FONT_HEIGHT;
                }
                if self.y_pos + FONT_HEIGHT > self.height {
                    self.scroll();
                }
                self.draw_char(c, self.x_pos, self.y_pos);
                self.x_pos += FONT_WIDTH;
            }
        }
        if self.y_pos + FONT_HEIGHT > self.height {
            self.scroll();
        }
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut console = Console::new();
    console.init();
    
    let welcome = b"[Console Driver] Started successfully in Ring 3!\n";
    for &b in welcome.iter() {
        console.write_char(b);
    }

    loop {
        // Wait for an IPC print message
        let msg = receive();
        if msg.msg_type == MSG_PRINT_CHAR {
            console.write_char(msg.arg1 as u8);
        }
    }
}

