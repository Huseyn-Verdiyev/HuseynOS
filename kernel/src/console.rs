use spin::Mutex;
use limine::framebuffer::Framebuffer;
use crate::vga_font;

/// Standard VT100 colors (RGB)
pub const COLOR_BLACK: u32 = 0x000000;
pub const COLOR_WHITE: u32 = 0xFFFFFF;
pub const COLOR_GRAY: u32  = 0xAAAAAA;
pub const COLOR_RED: u32   = 0xFF5555;
pub const COLOR_GREEN: u32 = 0x55FF55;
pub const COLOR_BLUE: u32  = 0x5555FF;

const FONT_WIDTH: usize = 8;
const FONT_HEIGHT: usize = 16;
const TAB_SIZE: usize = 4;

/// Global console instance
pub static CONSOLE: Mutex<Console> = Mutex::new(Console::new());

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

unsafe impl Send for Console {}
unsafe impl Sync for Console {}

impl Console {
    const fn new() -> Self {
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

    /// Initialize the console with the Limine framebuffer.
    pub fn init(&mut self, fb: &Framebuffer) {
        self.fb_ptr = fb.addr() as *mut u8;
        // The bpp from limine might be in bits. We need it in bytes for plotting.
        // Assuming 32-bit (4 bytes) per pixel.
        self.bpp = (fb.bpp() / 8) as usize;
        self.width = fb.width() as usize;
        self.height = fb.height() as usize;
        self.pitch = fb.pitch() as usize;

        self.clear();
    }

    /// Set foreground and background colors.
    pub fn set_colors(&mut self, fg: u32, bg: u32) {
        self.fg_color = fg;
        self.bg_color = bg;
    }

    /// Clear the screen.
    pub fn clear(&mut self) {
        if self.fb_ptr.is_null() { return; }

        for y in 0..self.height {
            for x in 0..self.width {
                self.plot_pixel(x, y, self.bg_color);
            }
        }
        self.x_pos = 0;
        self.y_pos = 0;
    }

    /// Plot a single pixel color.
    fn plot_pixel(&self, x: usize, y: usize, color: u32) {
        if self.fb_ptr.is_null() || x >= self.width || y >= self.height { return; }
        
        let offset = y * self.pitch + x * self.bpp;
        unsafe {
            let pixel_ptr = self.fb_ptr.add(offset) as *mut u32;
            core::ptr::write_volatile(pixel_ptr, color);
        }
    }

    /// Draw a character with the 8x16 font.
    fn draw_char(&mut self, c: u8, x: usize, y: usize) {
        let glyph = vga_font::get_char_data(c);

        for (row_idx, &row_mask) in glyph.iter().enumerate() {
            for col_idx in 0..FONT_WIDTH {
                // If the bit at col_idx is set (MSB first)
                if (row_mask & (0x80 >> col_idx)) != 0 {
                    self.plot_pixel(x + col_idx, y + row_idx, self.fg_color);
                } else {
                    self.plot_pixel(x + col_idx, y + row_idx, self.bg_color);
                }
            }
        }
    }

    /// Scroll the screen up by one character line.
    fn scroll(&mut self) {
        if self.fb_ptr.is_null() { return; }

        let bytes_per_line = self.pitch * FONT_HEIGHT;
        let total_bytes = self.pitch * self.height;

        unsafe {
            // Move everything up
            core::ptr::copy(
                self.fb_ptr.add(bytes_per_line),
                self.fb_ptr,
                total_bytes - bytes_per_line,
            );

            // Clear the last line
            let last_line_ptr = self.fb_ptr.add(total_bytes - bytes_per_line);
            core::ptr::write_bytes(last_line_ptr, 0, bytes_per_line); // Fast clear
        }

        self.y_pos = self.height - FONT_HEIGHT;
    }

    /// Write a character and advance cursor.
    pub fn write_char(&mut self, c: u8) {
        if self.fb_ptr.is_null() { return; }

        match c {
            b'\n' => self.newline(),
            b'\r' => self.x_pos = 0,
            b'\t' => {
                let spaces = TAB_SIZE - ((self.x_pos / FONT_WIDTH) % TAB_SIZE);
                for _ in 0..spaces { self.write_char(b' '); }
            }
            0x08 => self.backspace(), // Backspace
            _ => {
                if self.x_pos + FONT_WIDTH >= self.width {
                    self.newline();
                }

                self.draw_char(c, self.x_pos, self.y_pos);
                self.x_pos += FONT_WIDTH;
            }
        }
    }

    /// Write a newline.
    fn newline(&mut self) {
        self.x_pos = 0;
        self.y_pos += FONT_HEIGHT;
        if self.y_pos + FONT_HEIGHT > self.height {
            self.scroll();
        }
    }

    /// Handle backspace (delete previous char).
    fn backspace(&mut self) {
        if self.x_pos >= FONT_WIDTH {
            self.x_pos -= FONT_WIDTH;
            self.draw_char(b' ', self.x_pos, self.y_pos);
        } else if self.y_pos >= FONT_HEIGHT {
            self.y_pos -= FONT_HEIGHT;
            self.x_pos = self.width - FONT_WIDTH; // Best effort wrap
            self.draw_char(b' ', self.x_pos, self.y_pos);
        }
    }

    /// Write a string to the console.
    pub fn write_str(&mut self, s: &str) {
        for b in s.bytes() {
            self.write_char(b);
        }
    }
}

// Add formatting support so we can use `write!` macros
use core::fmt;
impl fmt::Write for Console {
    fn write_str(&mut self, s: &str) -> fmt::Result {
        self.write_str(s);
        Ok(())
    }
}

/// Print to both Serial and Console.
#[macro_export]
macro_rules! println {
    () => {
        $crate::serial_println!();
        {
            use core::fmt::Write;
            let _ = write!($crate::console::CONSOLE.lock(), "\n");
        }
    };
    ($($arg:tt)*) => {
        $crate::serial_println!($($arg)*);
        {
            use core::fmt::Write;
            let _ = writeln!($crate::console::CONSOLE.lock(), $($arg)*);
        }
    };
}

#[macro_export]
macro_rules! print {
    ($($arg:tt)*) => {
        $crate::serial_print!($($arg)*);
        {
            use core::fmt::Write;
            let _ = write!($crate::console::CONSOLE.lock(), $($arg)*);
        }
    };
}
