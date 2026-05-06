#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use libhuseyn::{get_fb_info, map_physical, ipc};

// ─── Layout Constants ───

const CURSOR_SIZE: usize = 16;
const TASKBAR_HEIGHT: usize = 32;
const TITLE_BAR_HEIGHT: usize = 24;
const BORDER_WIDTH: usize = 1;
const CLOSE_BTN_SIZE: usize = 16;
const CLOSE_BTN_MARGIN: usize = 4;

// ─── Color Palette (Premium Dark Theme) ───

// Wallpaper gradient endpoints
const WALL_TOP_R: u8 = 10;   const WALL_TOP_G: u8 = 10;   const WALL_TOP_B: u8 = 30;
const WALL_BOT_R: u8 = 40;   const WALL_BOT_G: u8 = 20;   const WALL_BOT_B: u8 = 80;

// Taskbar
const COLOR_TASKBAR_TOP: u32 = 0xFF1A1A2E;
const COLOR_TASKBAR_LINE: u32 = 0xFF4A4A6A;
const COLOR_POWER_BG: u32 = 0xFF8B0000;     // Dark red

// Window: Active
const COLOR_TITLE_ACTIVE_TOP: u32 = 0xFF2D5A8A;   // Steel blue
const COLOR_TITLE_ACTIVE_BOT: u32 = 0xFF1A3A5C;
const COLOR_TITLE_TEXT_ACTIVE: u32 = 0xFFFFFFFF;
const COLOR_BORDER_ACTIVE: u32 = 0xFF4A90D9;

// Window: Inactive
const COLOR_TITLE_INACTIVE_TOP: u32 = 0xFF3A3A3A;
const COLOR_TITLE_INACTIVE_BOT: u32 = 0xFF2A2A2A;
const COLOR_TITLE_TEXT_INACTIVE: u32 = 0xFF888888;
const COLOR_BORDER_INACTIVE: u32 = 0xFF555555;

// Close button
const COLOR_CLOSE_BG: u32 = 0xFFCC3333;
const COLOR_CLOSE_X: u32 = 0xFFFFFFFF;

// Cursor
const COLOR_CURSOR: u32 = 0xFFFFFFFF;
const COLOR_CURSOR_BORDER: u32 = 0xFF000000;

// Clock / text
const COLOR_CLOCK: u32 = 0xFFE0E0FF;
const COLOR_POWER_TEXT: u32 = 0xFFFFFFFF;

// 16x16 mouse cursor bitmap
static CURSOR_BITMAP: [[u8; 16]; 16] = [
    [2,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,2,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,1,2,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,1,1,2,0,0,0,0,0,0,0,0,0,0,0,0],
    [2,1,1,1,2,0,0,0,0,0,0,0,0,0,0,0],
    [2,1,1,1,1,2,0,0,0,0,0,0,0,0,0,0],
    [2,1,1,1,1,1,2,0,0,0,0,0,0,0,0,0],
    [2,1,1,1,1,1,1,2,0,0,0,0,0,0,0,0],
    [2,1,1,1,1,1,1,1,2,0,0,0,0,0,0,0],
    [2,1,1,1,1,1,1,1,1,2,0,0,0,0,0,0],
    [2,1,1,1,1,1,2,2,2,2,2,0,0,0,0,0],
    [2,1,1,2,1,1,2,0,0,0,0,0,0,0,0,0],
    [2,1,2,0,2,1,1,2,0,0,0,0,0,0,0,0],
    [2,2,0,0,2,1,1,2,0,0,0,0,0,0,0,0],
    [2,0,0,0,0,2,1,1,2,0,0,0,0,0,0,0],
    [0,0,0,0,0,2,2,2,2,0,0,0,0,0,0,0],
];

// ─── Window Management ───

struct Window {
    id: u64,
    owner_pid: usize,     // PID of the process that created this window
    x: usize,
    y: usize,
    width: usize,         // Client area width
    height: usize,        // Client area height
    #[allow(dead_code)]
    shm_id: u32,
    buffer: *const u32,   // Mapped SHM buffer (client pixels only)
    title: [u8; 32],      // Window title (ASCII)
    title_len: usize,
}

impl Window {
    /// Total width including borders
    fn total_width(&self) -> usize {
        self.width + 2 * BORDER_WIDTH
    }
    /// Total height including title bar and borders
    fn total_height(&self) -> usize {
        self.height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH
    }
    /// Screen coordinates of the client area (where pixels from SHM are drawn)
    fn client_x(&self) -> usize { self.x + BORDER_WIDTH }
    fn client_y(&self) -> usize { self.y + TITLE_BAR_HEIGHT + BORDER_WIDTH }

    /// Check if a point is inside the title bar
    fn hit_title_bar(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.total_width()
        && my >= self.y + BORDER_WIDTH && my < self.y + BORDER_WIDTH + TITLE_BAR_HEIGHT
    }

    /// Check if a point is inside the close button
    fn hit_close_button(&self, mx: usize, my: usize) -> bool {
        let btn_x = self.x + self.total_width() - BORDER_WIDTH - CLOSE_BTN_MARGIN - CLOSE_BTN_SIZE;
        let btn_y = self.y + BORDER_WIDTH + (TITLE_BAR_HEIGHT - CLOSE_BTN_SIZE) / 2;
        mx >= btn_x && mx < btn_x + CLOSE_BTN_SIZE
        && my >= btn_y && my < btn_y + CLOSE_BTN_SIZE
    }

    /// Check if a point is anywhere inside the full window (including decorations)
    fn hit_anywhere(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.total_width()
        && my >= self.y && my < self.y + self.total_height()
    }
}

// ─── Drag State ───

struct DragState {
    active: bool,
    window_id: u64,
    offset_x: i32,
    offset_y: i32,
}

// ─── Compositor ───

struct Compositor {
    fb_ptr: *mut u32,
    backbuf: Vec<u32>,
    bg_cache: Vec<u32>,   // Pre-rendered wallpaper + taskbar (static background)
    width: usize,
    height: usize,
    pitch: usize,
    bpp: usize,
    mouse_x: i32,
    mouse_y: i32,
    mouse_buttons: u8,
    dirty: bool,
    windows: Vec<Window>,
    next_window_id: u64,
    alloc_vaddr: u64,
    drag: DragState,
    frame_count: u32,
}

impl Compositor {
    fn new() -> Self {
        Self {
            fb_ptr: core::ptr::null_mut(),
            backbuf: Vec::new(),
            bg_cache: Vec::new(),
            width: 0,
            height: 0,
            pitch: 0,
            bpp: 0,
            mouse_x: 512,
            mouse_y: 384,
            mouse_buttons: 0,
            dirty: true,
            windows: Vec::new(),
            next_window_id: 1,
            alloc_vaddr: 0xB000_0000_u64,
            drag: DragState { active: false, window_id: 0, offset_x: 0, offset_y: 0 },
            frame_count: 0,
        }
    }

    fn init(&mut self) {
        let (paddr, w, h, p, bpp) = get_fb_info();
        self.width = w as usize;
        self.height = h as usize;
        self.pitch = p as usize;
        self.bpp = bpp as usize;

        let fb_virt = 0xA000_0000_u64;
        let fb_size = (self.pitch * self.height) as u64;
        if map_physical(fb_virt, paddr, fb_size).is_err() {
            libhuseyn::exit(1);
        }
        self.fb_ptr = fb_virt as *mut u32;

        let pixel_count = self.width * self.height;
        self.backbuf = vec![0u32; pixel_count];
        self.bg_cache = vec![0u32; pixel_count];

        // Pre-render the static background (wallpaper + taskbar) into bg_cache
        self.prerender_background();
    }

    /// Pre-compute the wallpaper gradient + taskbar into bg_cache.
    /// Called once at init. The clock text is NOT part of this cache
    /// since it changes every minute.
    fn prerender_background(&mut self) {
        let h = self.height.saturating_sub(TASKBAR_HEIGHT);
        let w = self.width;

        // Wallpaper gradient
        for y in 0..h {
            let t = y as i32 * 255 / h.max(1) as i32;
            let r = (WALL_TOP_R as i32 + (WALL_BOT_R as i32 - WALL_TOP_R as i32) * t / 255).clamp(0, 255) as u32;
            let g = (WALL_TOP_G as i32 + (WALL_BOT_G as i32 - WALL_TOP_G as i32) * t / 255).clamp(0, 255) as u32;
            let b = (WALL_TOP_B as i32 + (WALL_BOT_B as i32 - WALL_TOP_B as i32) * t / 255).clamp(0, 255) as u32;
            let color = 0xFF000000 | (r << 16) | (g << 8) | b;
            let row_start = y * w;
            for x in 0..w {
                self.bg_cache[row_start + x] = color;
            }
        }

        // Taskbar gradient
        let tb_y = self.height.saturating_sub(TASKBAR_HEIGHT);
        for dy in 0..TASKBAR_HEIGHT {
            let color = Self::lerp_color(COLOR_TASKBAR_TOP, 0xFF222240, dy as u32, TASKBAR_HEIGHT as u32);
            let row_start = (tb_y + dy) * w;
            for x in 0..w {
                if tb_y + dy < self.height {
                    self.bg_cache[row_start + x] = color;
                }
            }
        }

        // Taskbar top border accent
        if tb_y < self.height {
            let row_start = tb_y * w;
            for x in 0..w {
                self.bg_cache[row_start + x] = COLOR_TASKBAR_LINE;
            }
        }

        // "HuseynOS" label (static, rendered into bg_cache)
        let mut cache = core::mem::take(&mut self.bg_cache);
        self.draw_text_into(&mut cache, 10, tb_y + 10, "HuseynOS", COLOR_CLOCK);
        self.bg_cache = cache;
    }

    /// Draw text into an arbitrary buffer (not self.backbuf)
    fn draw_text_into(&self, buf: &mut [u32], mut x: usize, y: usize, text: &str, color: u32) {
        let w = self.width;
        let h = self.height;
        for ch in text.bytes() {
            let glyph = get_mini_glyph(ch);
            for (row, &bits) in glyph.iter().enumerate() {
                let py = y + row;
                if py >= h { continue; }
                for col in 0..5 {
                    if (bits >> (4 - col)) & 1 != 0 {
                        let px = x + col;
                        if px < w {
                            buf[py * w + px] = color;
                        }
                    }
                }
            }
            x += 7;
        }
    }

    // ─── Drawing Primitives ───

    #[inline]
    fn set_pixel(&mut self, x: usize, y: usize, color: u32) {
        if x < self.width && y < self.height {
            self.backbuf[y * self.width + x] = color;
        }
    }

    fn fill_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        for dy in 0..h {
            let py = y + dy;
            if py >= self.height { break; }
            for dx in 0..w {
                let px = x + dx;
                if px >= self.width { break; }
                self.backbuf[py * self.width + px] = color;
            }
        }
    }

    /// Linear interpolation between two colors
    fn lerp_color(c1: u32, c2: u32, t: u32, max: u32) -> u32 {
        if max == 0 { return c1; }
        let r1 = ((c1 >> 16) & 0xFF) as i32;
        let g1 = ((c1 >> 8) & 0xFF) as i32;
        let b1 = (c1 & 0xFF) as i32;
        let r2 = ((c2 >> 16) & 0xFF) as i32;
        let g2 = ((c2 >> 8) & 0xFF) as i32;
        let b2 = (c2 & 0xFF) as i32;
        let t = t as i32;
        let max = max as i32;
        let r = (r1 + (r2 - r1) * t / max).clamp(0, 255) as u32;
        let g = (g1 + (g2 - g1) * t / max).clamp(0, 255) as u32;
        let b = (b1 + (b2 - b1) * t / max).clamp(0, 255) as u32;
        0xFF000000 | (r << 16) | (g << 8) | b
    }

    // ─── Scene Drawing ───

    fn restore_background(&mut self) {
        // Fast memcpy of pre-rendered background
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.bg_cache.as_ptr(),
                self.backbuf.as_mut_ptr(),
                self.bg_cache.len(),
            );
        }
    }

    fn draw_taskbar_dynamic(&mut self) {
        let tb_y = self.height.saturating_sub(TASKBAR_HEIGHT);

        // Only draw dynamic parts: Clock + Power button
        let dt = libhuseyn::get_time();
        let mut clock_buf = [0u8; 5];
        clock_buf[0] = b'0' + (dt.hour / 10);
        clock_buf[1] = b'0' + (dt.hour % 10);
        clock_buf[2] = b':';
        clock_buf[3] = b'0' + (dt.minute / 10);
        clock_buf[4] = b'0' + (dt.minute % 10);
        let clock_x = self.width - 100;
        if let Ok(clock_str) = core::str::from_utf8(&clock_buf) {
            self.draw_text(clock_x, tb_y + 10, clock_str, COLOR_CLOCK);
        }

        // Power button
        let pw_x = self.width - 35;
        let pw_y = tb_y + 6;
        self.fill_rect(pw_x, pw_y, 24, 20, COLOR_POWER_BG);
        self.draw_text(pw_x + 7, pw_y + 6, "X", COLOR_POWER_TEXT);
    }

    /// Returns true if (mx, my) is inside the taskbar power button
    fn hit_power_button(&self, mx: usize, my: usize) -> bool {
        let tb_y = self.height.saturating_sub(TASKBAR_HEIGHT);
        let pw_x = self.width - 35;
        let pw_y = tb_y + 6;
        mx >= pw_x && mx < pw_x + 24 && my >= pw_y && my < pw_y + 20
    }

    fn draw_window_decorations(&mut self, win_idx: usize) {
        let is_active = win_idx == self.windows.len() - 1;

        // Extract all values we need from the window before borrowing self mutably
        let win_x = self.windows[win_idx].x;
        let win_y = self.windows[win_idx].y;
        let win_width = self.windows[win_idx].width;
        let total_w = self.windows[win_idx].total_width();
        let total_h = self.windows[win_idx].total_height();
        let title_len = self.windows[win_idx].title_len;
        let mut title_copy = [0u8; 32];
        title_copy.copy_from_slice(&self.windows[win_idx].title);

        let border_color = if is_active { COLOR_BORDER_ACTIVE } else { COLOR_BORDER_INACTIVE };
        let title_top = if is_active { COLOR_TITLE_ACTIVE_TOP } else { COLOR_TITLE_INACTIVE_TOP };
        let title_bot = if is_active { COLOR_TITLE_ACTIVE_BOT } else { COLOR_TITLE_INACTIVE_BOT };
        let title_text_color = if is_active { COLOR_TITLE_TEXT_ACTIVE } else { COLOR_TITLE_TEXT_INACTIVE };

        // 1. Border (1px all around)
        self.fill_rect(win_x, win_y, total_w, BORDER_WIDTH, border_color);
        self.fill_rect(win_x, win_y + total_h - BORDER_WIDTH, total_w, BORDER_WIDTH, border_color);
        self.fill_rect(win_x, win_y, BORDER_WIDTH, total_h, border_color);
        self.fill_rect(win_x + total_w - BORDER_WIDTH, win_y, BORDER_WIDTH, total_h, border_color);

        // 2. Title bar (gradient)
        let tb_x = win_x + BORDER_WIDTH;
        let tb_y = win_y + BORDER_WIDTH;
        for dy in 0..TITLE_BAR_HEIGHT {
            let color = Self::lerp_color(title_top, title_bot, dy as u32, TITLE_BAR_HEIGHT as u32);
            for dx in 0..win_width {
                self.set_pixel(tb_x + dx, tb_y + dy, color);
            }
        }

        // 3. Title text
        let text = if title_len > 0 {
            unsafe { core::str::from_utf8_unchecked(&title_copy[..title_len]) }
        } else {
            "Window"
        };
        self.draw_text(tb_x + 8, tb_y + 8, text, title_text_color);

        // 4. Close button [X]
        let btn_x = win_x + total_w - BORDER_WIDTH - CLOSE_BTN_MARGIN - CLOSE_BTN_SIZE;
        let btn_y = win_y + BORDER_WIDTH + (TITLE_BAR_HEIGHT - CLOSE_BTN_SIZE) / 2;
        self.fill_rect(btn_x, btn_y, CLOSE_BTN_SIZE, CLOSE_BTN_SIZE, COLOR_CLOSE_BG);
        for i in 0..CLOSE_BTN_SIZE {
            self.set_pixel(btn_x + i, btn_y + i, COLOR_CLOSE_X);
            self.set_pixel(btn_x + CLOSE_BTN_SIZE - 1 - i, btn_y + i, COLOR_CLOSE_X);
        }
    }

    fn draw_windows(&mut self) {
        let _win_count = self.windows.len();
        // Collect window data to avoid borrow issues
        let mut win_data = Vec::new();
        for win in &self.windows {
            win_data.push((win.client_x(), win.client_y(), win.width, win.height, win.buffer));
        }

        // Draw decorations + client area bottom-to-top
        for (idx, (cx, cy, cw, ch, buf)) in win_data.iter().enumerate() {
            // Draw decorations first
            self.draw_window_decorations(idx);

            // Draw client area pixels from SHM
            for y in 0..*ch {
                let screen_y = cy + y;
                if screen_y >= self.height { break; }
                let row_offset = y * cw;
                for x in 0..*cw {
                    let screen_x = cx + x;
                    if screen_x >= self.width { break; }
                    unsafe {
                        let pixel = *buf.add(row_offset + x);
                        if (pixel >> 24) != 0 {
                            self.set_pixel(screen_x, screen_y, pixel);
                        }
                    }
                }
            }
        }
    }

    fn draw_cursor(&mut self) {
        let cx = self.mouse_x as usize;
        let cy = self.mouse_y as usize;
        for dy in 0..CURSOR_SIZE {
            for dx in 0..CURSOR_SIZE {
                match CURSOR_BITMAP[dy][dx] {
                    1 => self.set_pixel(cx + dx, cy + dy, COLOR_CURSOR),
                    2 => self.set_pixel(cx + dx, cy + dy, COLOR_CURSOR_BORDER),
                    _ => {}
                }
            }
        }
    }

    // ─── Render ───

    fn render(&mut self) {
        self.restore_background();    // Fast memcpy of cached wallpaper+taskbar
        self.draw_taskbar_dynamic();  // Only clock + power button (dynamic)
        self.draw_windows();
        self.draw_cursor();
        self.blit();
        self.dirty = false;
        self.frame_count = self.frame_count.wrapping_add(1);
    }

    fn blit(&self) {
        let stride_pixels = self.pitch / 4;
        unsafe {
            for y in 0..self.height {
                let src = &self.backbuf[y * self.width] as *const u32;
                let dst = self.fb_ptr.add(y * stride_pixels);
                core::ptr::copy_nonoverlapping(src, dst, self.width);
            }
        }
    }

    // ─── Text Rendering ───

    fn draw_text(&mut self, mut x: usize, y: usize, text: &str, color: u32) {
        for ch in text.bytes() {
            self.draw_mini_char(x, y, ch, color);
            x += 7;
        }
    }

    fn draw_mini_char(&mut self, x: usize, y: usize, ch: u8, color: u32) {
        let glyph = get_mini_glyph(ch);
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..5 {
                if (bits >> (4 - col)) & 1 != 0 {
                    self.set_pixel(x + col, y + row, color);
                }
            }
        }
    }

    // ─── Event Handling ───

    fn handle_message(&mut self, msg: &ipc::Message) {
        match msg.msg_type {
            ipc::MSG_MOUSE_MOVE => {
                let new_x = msg.arg1 as i32;
                let new_y = msg.arg2 as i32;
                self.mouse_buttons = msg.arg3 as u8;

                // Handle dragging
                if self.drag.active && (self.mouse_buttons & 1 != 0) {
                    // Find the window being dragged
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == self.drag.window_id) {
                        let new_win_x = new_x - self.drag.offset_x;
                        let new_win_y = new_y - self.drag.offset_y;
                        // Clamp to screen bounds
                        win.x = (new_win_x.max(0) as usize).min(self.width.saturating_sub(50));
                        win.y = (new_win_y.max(0) as usize).min(self.height.saturating_sub(TASKBAR_HEIGHT + 10));
                    }
                } else if self.drag.active {
                    // Button released, stop dragging
                    self.drag.active = false;
                }

                self.mouse_x = new_x;
                self.mouse_y = new_y;
                self.dirty = true;
            }
            ipc::MSG_MOUSE_CLICK => {
                let prev = self.mouse_buttons;
                self.mouse_buttons = msg.arg3 as u8;
                let just_pressed = (self.mouse_buttons & 1 != 0) && (prev & 1 == 0);
                self.dirty = true;

                if just_pressed {
                    let mx = self.mouse_x as usize;
                    let my = self.mouse_y as usize;

                    // 1. Check Power button first
                    if self.hit_power_button(mx, my) {
                        libhuseyn::shutdown();
                    }

                    // 2. Check windows in reverse Z-order (top first)
                    let mut action: Option<(usize, bool, bool)> = None; // (idx, is_close, is_titlebar)
                    for i in (0..self.windows.len()).rev() {
                        let win = &self.windows[i];
                        if win.hit_close_button(mx, my) {
                            action = Some((i, true, false));
                            break;
                        }
                        if win.hit_title_bar(mx, my) {
                            action = Some((i, false, true));
                            break;
                        }
                        if win.hit_anywhere(mx, my) {
                            action = Some((i, false, false));
                            break;
                        }
                    }

                    if let Some((idx, is_close, is_titlebar)) = action {
                        if is_close {
                            // Send MSG_QUIT to the window's owner PID
                            let owner = self.windows[idx].owner_pid;
                            let mut quit_msg = ipc::Message::empty();
                            quit_msg.msg_type = ipc::MSG_QUIT;
                            ipc::send(owner, &quit_msg);
                            // Remove the window from our list
                            self.windows.remove(idx);
                        } else if is_titlebar {
                            // Bring to front
                            if idx < self.windows.len() - 1 {
                                let win = self.windows.remove(idx);
                                self.windows.push(win);
                            }
                            // Start dragging (window is now last)
                            let win = self.windows.last().unwrap();
                            self.drag = DragState {
                                active: true,
                                window_id: win.id,
                                offset_x: mx as i32 - win.x as i32,
                                offset_y: my as i32 - win.y as i32,
                            };
                        } else {
                            // Just bring to front (clicked client area)
                            if idx < self.windows.len() - 1 {
                                let win = self.windows.remove(idx);
                                self.windows.push(win);
                            }
                        }
                    }
                }
            }
            ipc::MSG_WINDOW_CREATE => {
                let width = msg.arg1 as usize;
                let height = msg.arg2 as usize;
                let shm_id = msg.arg3 as u32;
                let owner_pid = msg.sender;

                let vaddr = self.alloc_vaddr;
                let bytes = (width * height * 4) as u64;
                let next_vaddr = (vaddr + bytes + 0xFFF) & !0xFFF;

                if libhuseyn::shm_map(shm_id, vaddr).is_ok() {
                    // Center the window on screen
                    let total_w = width + 2 * BORDER_WIDTH;
                    let total_h = height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH;
                    let center_x = self.width.saturating_sub(total_w) / 2;
                    let center_y = self.height.saturating_sub(TASKBAR_HEIGHT).saturating_sub(total_h) / 2;

                    let mut title = [0u8; 32];
                    let default_title = b"Terminal";
                    let len = default_title.len().min(32);
                    title[..len].copy_from_slice(&default_title[..len]);

                    let win = Window {
                        id: self.next_window_id,
                        owner_pid,
                        x: center_x + self.windows.len() * 20, // Slight cascade
                        y: center_y + self.windows.len() * 20,
                        width,
                        height,
                        shm_id,
                        buffer: vaddr as *const u32,
                        title,
                        title_len: len,
                    };
                    self.windows.push(win);
                    self.next_window_id += 1;
                    self.alloc_vaddr = next_vaddr;
                    self.dirty = true;
                }
            }
            ipc::MSG_WINDOW_DAMAGE => {
                self.dirty = true;
            }
            _ => {}
        }
    }
}

// ─── Minimal 5x7 Font ───

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
        _    => [0b11111, 0b10001, 0b10001, 0b10001, 0b10001, 0b10001, 0b11111],
    }
}

// ─── Entry Point ───

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut comp = Compositor::new();
    comp.init();
    comp.render();

    loop {
        // Block until at least one message arrives
        let msg = ipc::receive();
        comp.handle_message(&msg);

        // Drain ALL pending messages before rendering (batch processing)
        // This prevents rendering once per mouse-move event
        while let Some(msg) = ipc::try_receive() {
            comp.handle_message(&msg);
        }

        if comp.dirty {
            comp.render();
        }
    }
}
