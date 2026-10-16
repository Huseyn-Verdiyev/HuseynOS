#![no_std]
#![no_main]

extern crate alloc;
use alloc::vec;
use alloc::vec::Vec;
use libhuseyn::{get_fb_info, map_physical, ipc, spawn};

// ─── Layout Constants (HuseynOS Monolith UI) ───

const CURSOR_SIZE: usize = 16;
const TOPBAR_HEIGHT: usize = 24;
const TITLE_BAR_HEIGHT: usize = 20;
const BORDER_WIDTH: usize = 1;
const CLOSE_BTN_SIZE: usize = 12;
const CLOSE_BTN_MARGIN: usize = 4;

// ─── Color Palette (HuseynOS Cyber-Monolith Theme) ───
// Pure obsidian canvas, precision dot-grid, razor 1px cyan/slate borders, zero OS-clone clutter.

const COLOR_BG_OBSIDIAN: u32       = 0xFF080A0F; // Deep obsidian background
const COLOR_BG_GRID_DOT: u32       = 0xFF161D2A; // Subtle architectural grid dots
const COLOR_BG_CROSSHAIR: u32      = 0xFF1F293A; // Telemetry corner markers

// Top Monolith Bar
const COLOR_TOPBAR_BG: u32         = 0xFF0C1017;
const COLOR_TOPBAR_LINE: u32       = 0xFF1E293B;
const COLOR_ACCENT_CYAN: u32       = 0xFF00E5FF; // Primary signature accent
const COLOR_ACCENT_AMBER: u32      = 0xFFFFB000; // Secondary telemetry accent
const COLOR_PILL_BG: u32           = 0xFF131B26;
const COLOR_PILL_BORDER: u32       = 0xFF26354A;
const COLOR_POWER_BG: u32          = 0xFF2A1215;
const COLOR_POWER_BORDER: u32      = 0xFFFF3355;

// Window: Active
const COLOR_WIN_HEADER_ACTIVE: u32 = 0xFF0F1722;
const COLOR_BORDER_ACTIVE: u32     = 0xFF00E5FF;
const COLOR_TITLE_ACTIVE: u32      = 0xFFE2E8F0;

// Window: Inactive
const COLOR_WIN_HEADER_INACT: u32  = 0xFF0B0E14;
const COLOR_BORDER_INACTIVE: u32   = 0xFF222D3D;
const COLOR_TITLE_INACTIVE: u32    = 0xFF64748B;

// Text
const COLOR_TEXT_BRIGHT: u32       = 0xFFF1F5F9;
const COLOR_TEXT_MUTED: u32        = 0xFF94A3B8;
const COLOR_CLOSE_FG: u32          = 0xFFFF4466;

// Cursor
const COLOR_CURSOR_MAIN: u32       = 0xFF00E5FF;
const COLOR_CURSOR_CORE: u32       = 0xFFFFFFFF;
const COLOR_CURSOR_OUTLINE: u32    = 0xFF05070A;

// Precision geometric cursor bitmap (1 = white core, 2 = dark outline, 3 = cyan accent)
static CURSOR_BITMAP: [[u8; 16]; 16] = [
    [3,3,3,2,0,0,0,0,0,0,0,0,0,0,0,0],
    [3,1,1,3,2,0,0,0,0,0,0,0,0,0,0,0],
    [3,1,1,1,3,2,0,0,0,0,0,0,0,0,0,0],
    [2,3,1,1,1,3,2,0,0,0,0,0,0,0,0,0],
    [0,2,3,1,1,1,3,2,0,0,0,0,0,0,0,0],
    [0,0,2,3,1,1,1,3,2,0,0,0,0,0,0,0],
    [0,0,0,2,3,1,1,1,3,2,0,0,0,0,0,0],
    [0,0,0,0,2,3,1,1,1,3,2,0,0,0,0,0],
    [0,0,0,0,0,2,3,1,1,3,2,0,0,0,0,0],
    [0,0,0,0,0,0,2,3,1,2,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,2,3,0,3,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0,0,3,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
    [0,0,0,0,0,0,0,0,0,0,0,0,0,0,0,0],
];

// ─── Window Management ───

struct Window {
    id: u64,
    owner_pid: usize,
    x: usize,
    y: usize,
    float_x: usize,
    float_y: usize,
    width: usize,
    height: usize,
    shm_id: u32,
    buffer: *const u32,
    title: [u8; 32],
    title_len: usize,
}

impl Window {
    fn total_width(&self) -> usize {
        self.width + 2 * BORDER_WIDTH
    }
    fn total_height(&self) -> usize {
        self.height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH
    }
    fn client_x(&self) -> usize { self.x + BORDER_WIDTH }
    fn client_y(&self) -> usize { self.y + TITLE_BAR_HEIGHT + BORDER_WIDTH }

    fn hit_title_bar(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.total_width()
        && my >= self.y + BORDER_WIDTH && my < self.y + BORDER_WIDTH + TITLE_BAR_HEIGHT
    }

    fn hit_close_button(&self, mx: usize, my: usize) -> bool {
        let btn_x = self.x + self.total_width() - BORDER_WIDTH - CLOSE_BTN_MARGIN - CLOSE_BTN_SIZE;
        let btn_y = self.y + BORDER_WIDTH + (TITLE_BAR_HEIGHT - CLOSE_BTN_SIZE) / 2;
        mx >= btn_x && mx < btn_x + CLOSE_BTN_SIZE
        && my >= btn_y && my < btn_y + CLOSE_BTN_SIZE
    }

    fn hit_anywhere(&self, mx: usize, my: usize) -> bool {
        mx >= self.x && mx < self.x + self.total_width()
        && my >= self.y && my < self.y + self.total_height()
    }
}

struct DragState {
    active: bool,
    window_id: u64,
    offset_x: i32,
    offset_y: i32,
}

struct Compositor {
    fb_ptr: *mut u32,
    backbuf: Vec<u32>,
    bg_cache: Vec<u32>,
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
    tile_mode: bool,
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
            tile_mode: false,
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

        self.prerender_background();
    }

    fn prerender_background(&mut self) {
        let h = self.height;
        let w = self.width;

        // 1. Fill deep obsidian base
        for i in 0..(w * h) {
            self.bg_cache[i] = COLOR_BG_OBSIDIAN;
        }

        // 2. Architectural precision dot-matrix grid (every 24px)
        for y in (TOPBAR_HEIGHT + 16..h).step_by(24) {
            let row = y * w;
            for x in (16..w).step_by(24) {
                self.bg_cache[row + x] = COLOR_BG_GRID_DOT;
            }
        }

        // 3. Subtle engineering crosshairs at major intersections (every 120px)
        for y in (TOPBAR_HEIGHT + 64..h.saturating_sub(32)).step_by(120) {
            for x in (64..w.saturating_sub(32)).step_by(120) {
                for d in 0..5 {
                    let cx = x + d - 2;
                    let cy = y + d - 2;
                    if cx < w && y < h { self.bg_cache[y * w + cx] = COLOR_BG_CROSSHAIR; }
                    if x < w && cy < h { self.bg_cache[cy * w + x] = COLOR_BG_CROSSHAIR; }
                }
            }
        }

        // 4. Top Monolith Strip
        for y in 0..TOPBAR_HEIGHT {
            let row = y * w;
            for x in 0..w {
                self.bg_cache[row + x] = COLOR_TOPBAR_BG;
            }
        }
        if TOPBAR_HEIGHT < h {
            let line_row = (TOPBAR_HEIGHT - 1) * w;
            for x in 0..w {
                self.bg_cache[line_row + x] = COLOR_TOPBAR_LINE;
            }
            for x in 0..96.min(w) {
                self.bg_cache[line_row + x] = COLOR_ACCENT_CYAN;
            }
        }

        for y in 4..(TOPBAR_HEIGHT - 4) {
            for x in 8..11 {
                self.bg_cache[y * w + x] = COLOR_ACCENT_CYAN;
            }
        }

        let mut cache = core::mem::take(&mut self.bg_cache);
        self.draw_text_into(&mut cache, 16, 8, "HUSEYN//OS", COLOR_TEXT_BRIGHT);
        self.draw_text_into(&mut cache, 92, 8, "v1.0-MONOLITH", COLOR_TEXT_MUTED);

        if h > 24 {
            self.draw_text_into(&mut cache, 12, h - 14, "MICROKERNEL // X86_64 // ZERO-RING3-ISOLATION", COLOR_BG_CROSSHAIR);
        }
        self.bg_cache = cache;
    }

    fn draw_text_into(&self, buf: &mut [u32], mut x: usize, y: usize, text: &str, color: u32) {
        let w = self.width;
        let h = self.height;
        for ch in text.bytes() {
            let glyph = get_font_glyph(ch);
            for (row, &bits) in glyph.iter().enumerate() {
                let py = y + row;
                if py >= h { continue; }
                for col in 0..6 {
                    if (bits >> (5 - col)) & 1 != 0 {
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
            let row = py * self.width;
            for dx in 0..w {
                let px = x + dx;
                if px >= self.width { break; }
                self.backbuf[row + px] = color;
            }
        }
    }

    fn stroke_rect(&mut self, x: usize, y: usize, w: usize, h: usize, color: u32) {
        if w == 0 || h == 0 { return; }
        self.fill_rect(x, y, w, 1, color);
        self.fill_rect(x, y + h - 1, w, 1, color);
        self.fill_rect(x, y, 1, h, color);
        self.fill_rect(x + w - 1, y, 1, h, color);
    }

    fn restore_background(&mut self) {
        unsafe {
            core::ptr::copy_nonoverlapping(
                self.bg_cache.as_ptr(),
                self.backbuf.as_mut_ptr(),
                self.bg_cache.len(),
            );
        }
    }

    fn hit_spawn_term_button(&self, mx: usize, my: usize) -> bool {
        mx >= 200 && mx < 256 && my >= 4 && my < 20
    }

    fn hit_mode_button(&self, mx: usize, my: usize) -> bool {
        mx >= 264 && mx < 328 && my >= 4 && my < 20
    }

    fn hit_power_button(&self, mx: usize, my: usize) -> bool {
        let pw_x = self.width.saturating_sub(46);
        mx >= pw_x && mx < pw_x + 38 && my >= 4 && my < 20
    }

    fn apply_layout(&mut self) {
        let count = self.windows.len();
        if count == 0 { return; }

        if self.tile_mode {
            let pad = 12;
            let avail_w = self.width.saturating_sub(pad * 2);
            let avail_h = self.height.saturating_sub(TOPBAR_HEIGHT + pad * 2);

            for (i, win) in self.windows.iter_mut().enumerate() {
                let cell_w = avail_w / count;
                let cell_x = pad + i * cell_w;
                let cx = cell_x + (cell_w.saturating_sub(win.total_width())) / 2;
                let cy = TOPBAR_HEIGHT + pad + (avail_h.saturating_sub(win.total_height())) / 2;
                win.x = cx;
                win.y = cy;
            }
        } else {
            for win in self.windows.iter_mut() {
                win.x = win.float_x;
                win.y = win.float_y;
            }
        }
    }

    fn draw_topbar_dynamic(&mut self) {
        // 1. [+TERM] launcher button
        self.fill_rect(200, 4, 56, 16, COLOR_PILL_BG);
        self.stroke_rect(200, 4, 56, 16, COLOR_PILL_BORDER);
        self.draw_text(207, 8, "+TERM", COLOR_ACCENT_CYAN);

        // 2. [FLOAT / TILE] layout mode toggle button
        self.fill_rect(264, 4, 64, 16, COLOR_PILL_BG);
        self.stroke_rect(264, 4, 64, 16, if self.tile_mode { COLOR_ACCENT_AMBER } else { COLOR_PILL_BORDER });
        if self.tile_mode {
            self.draw_text(273, 8, "M:TILE", COLOR_ACCENT_AMBER);
        } else {
            self.draw_text(270, 8, "M:FLOAT", COLOR_TEXT_MUTED);
        }

        // 3. Active window telemetry indicator
        let mut task_buf = [b'T', b'A', b'S', b'K', b'S', b':', b'0'];
        let n = self.windows.len().min(9) as u8;
        task_buf[6] = b'0' + n;
        if let Ok(s) = core::str::from_utf8(&task_buf) {
            self.draw_text(340, 8, s, COLOR_TEXT_MUTED);
        }

        // 4. Real-Time CMOS Clock (HH:MM:SS)
        let dt = libhuseyn::get_time();
        let mut clock_buf = [0u8; 8];
        clock_buf[0] = b'0' + (dt.hour / 10);
        clock_buf[1] = b'0' + (dt.hour % 10);
        clock_buf[2] = b':';
        clock_buf[3] = b'0' + (dt.minute / 10);
        clock_buf[4] = b'0' + (dt.minute % 10);
        clock_buf[5] = b':';
        clock_buf[6] = b'0' + (dt.second / 10);
        clock_buf[7] = b'0' + (dt.second % 10);

        let clock_x = self.width.saturating_sub(114);
        if let Ok(clock_str) = core::str::from_utf8(&clock_buf) {
            self.draw_text(clock_x, 8, clock_str, COLOR_TEXT_BRIGHT);
        }

        // 5. Minimalist [PWR] button
        let pw_x = self.width.saturating_sub(46);
        self.fill_rect(pw_x, 4, 38, 16, COLOR_POWER_BG);
        self.stroke_rect(pw_x, 4, 38, 16, COLOR_POWER_BORDER);
        self.draw_text(pw_x + 9, 8, "PWR", COLOR_CLOSE_FG);
    }

    fn draw_window_decorations(&mut self, win_idx: usize) {
        let is_active = win_idx == self.windows.len() - 1;

        let win_x = self.windows[win_idx].x;
        let win_y = self.windows[win_idx].y;
        let win_width = self.windows[win_idx].width;
        let total_w = self.windows[win_idx].total_width();
        let total_h = self.windows[win_idx].total_height();
        let owner_pid = self.windows[win_idx].owner_pid;
        let title_len = self.windows[win_idx].title_len;
        let mut title_copy = [0u8; 32];
        title_copy.copy_from_slice(&self.windows[win_idx].title);

        let border_color = if is_active { COLOR_BORDER_ACTIVE } else { COLOR_BORDER_INACTIVE };
        let header_bg = if is_active { COLOR_WIN_HEADER_ACTIVE } else { COLOR_WIN_HEADER_INACT };
        let title_color = if is_active { COLOR_TITLE_ACTIVE } else { COLOR_TITLE_INACTIVE };

        self.stroke_rect(win_x, win_y, total_w, total_h, border_color);

        let tb_x = win_x + BORDER_WIDTH;
        let tb_y = win_y + BORDER_WIDTH;
        self.fill_rect(tb_x, tb_y, win_width, TITLE_BAR_HEIGHT - 1, header_bg);
        self.fill_rect(tb_x, tb_y + TITLE_BAR_HEIGHT - 1, win_width, 1, border_color);

        let accent = if is_active { COLOR_ACCENT_CYAN } else { COLOR_BORDER_INACTIVE };
        self.fill_rect(tb_x + 6, tb_y + 6, 4, 7, accent);

        let text = if title_len > 0 {
            unsafe { core::str::from_utf8_unchecked(&title_copy[..title_len]) }
        } else {
            "node"
        };
        self.draw_text(tb_x + 15, tb_y + 6, text, title_color);

        let d1 = (owner_pid / 10) as u8;
        let d0 = (owner_pid % 10) as u8;
        let tag_x = tb_x + 15 + (text.len() + 1) * 7;
        if d1 > 0 {
            let pid_tag = [b'[', b'P', b'I', b'D', b':', b'0' + d1, b'0' + d0, b']'];
            if let Ok(pid_str) = core::str::from_utf8(&pid_tag) {
                self.draw_text(tag_x, tb_y + 6, pid_str, COLOR_TEXT_MUTED);
            }
        } else {
            let pid_tag = [b'[', b'P', b'I', b'D', b':', b'0' + d0, b']'];
            if let Ok(pid_str) = core::str::from_utf8(&pid_tag) {
                self.draw_text(tag_x, tb_y + 6, pid_str, COLOR_TEXT_MUTED);
            }
        }

        let btn_x = win_x + total_w - BORDER_WIDTH - CLOSE_BTN_MARGIN - CLOSE_BTN_SIZE;
        let btn_y = win_y + BORDER_WIDTH + (TITLE_BAR_HEIGHT - CLOSE_BTN_SIZE) / 2;
        self.stroke_rect(btn_x, btn_y, CLOSE_BTN_SIZE, CLOSE_BTN_SIZE, if is_active { COLOR_CLOSE_FG } else { COLOR_BORDER_INACTIVE });
        for i in 3..(CLOSE_BTN_SIZE - 3) {
            self.set_pixel(btn_x + i, btn_y + i, COLOR_CLOSE_FG);
            self.set_pixel(btn_x + CLOSE_BTN_SIZE - 1 - i, btn_y + i, COLOR_CLOSE_FG);
        }
    }

    fn draw_windows(&mut self) {
        let mut win_data = Vec::new();
        for win in &self.windows {
            win_data.push((win.client_x(), win.client_y(), win.width, win.height, win.buffer));
        }

        for (idx, (cx, cy, cw, ch, buf)) in win_data.iter().enumerate() {
            self.draw_window_decorations(idx);

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
                    1 => self.set_pixel(cx + dx, cy + dy, COLOR_CURSOR_CORE),
                    2 => self.set_pixel(cx + dx, cy + dy, COLOR_CURSOR_OUTLINE),
                    3 => self.set_pixel(cx + dx, cy + dy, COLOR_CURSOR_MAIN),
                    _ => {}
                }
            }
        }
    }

    fn render(&mut self) {
        self.restore_background();
        self.draw_topbar_dynamic();
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

    fn draw_text(&mut self, mut x: usize, y: usize, text: &str, color: u32) {
        for ch in text.bytes() {
            self.draw_char(x, y, ch, color);
            x += 7;
        }
    }

    fn draw_char(&mut self, x: usize, y: usize, ch: u8, color: u32) {
        let glyph = get_font_glyph(ch);
        for (row, &bits) in glyph.iter().enumerate() {
            for col in 0..6 {
                if (bits >> (5 - col)) & 1 != 0 {
                    self.set_pixel(x + col, y + row, color);
                }
            }
        }
    }

    fn handle_message(&mut self, msg: &ipc::Message) {
        match msg.msg_type {
            ipc::MSG_MOUSE_MOVE => {
                let new_x = msg.arg1 as i32;
                let new_y = msg.arg2 as i32;
                self.mouse_buttons = msg.arg3 as u8;

                if self.drag.active && (self.mouse_buttons & 1 != 0) && !self.tile_mode {
                    if let Some(win) = self.windows.iter_mut().find(|w| w.id == self.drag.window_id) {
                        let new_win_x = new_x - self.drag.offset_x;
                        let new_win_y = new_y - self.drag.offset_y;
                        let clamped_x = (new_win_x.max(0) as usize).min(self.width.saturating_sub(60));
                        let clamped_y = (new_win_y.max(TOPBAR_HEIGHT as i32) as usize).min(self.height.saturating_sub(30));
                        win.x = clamped_x;
                        win.y = clamped_y;
                        win.float_x = clamped_x;
                        win.float_y = clamped_y;
                    }
                } else if self.drag.active {
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

                    // 1. Topbar controls
                    if self.hit_power_button(mx, my) {
                        libhuseyn::shutdown();
                    }
                    if self.hit_spawn_term_button(mx, my) {
                        let _ = spawn("term.elf");
                        return;
                    }
                    if self.hit_mode_button(mx, my) {
                        self.tile_mode = !self.tile_mode;
                        self.apply_layout();
                        return;
                    }

                    // 2. Window interactions (reverse Z-order)
                    let mut action: Option<(usize, bool, bool)> = None;
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
                            let owner = self.windows[idx].owner_pid;
                            let shm_id = self.windows[idx].shm_id;
                            let buf_vaddr = self.windows[idx].buffer as u64;
                            let mut quit_msg = ipc::Message::empty();
                            quit_msg.msg_type = ipc::MSG_QUIT;
                            ipc::send(owner, &quit_msg);
                            let _ = libhuseyn::shm_unmap(shm_id, buf_vaddr);
                            self.windows.remove(idx);
                            if self.tile_mode {
                                self.apply_layout();
                            }
                        } else if is_titlebar {
                            if idx < self.windows.len() - 1 {
                                let win = self.windows.remove(idx);
                                self.windows.push(win);
                            }
                            if !self.tile_mode {
                                let win = self.windows.last().unwrap();
                                self.drag = DragState {
                                    active: true,
                                    window_id: win.id,
                                    offset_x: mx as i32 - win.x as i32,
                                    offset_y: my as i32 - win.y as i32,
                                };
                            }
                        } else if idx < self.windows.len() - 1 {
                            let win = self.windows.remove(idx);
                            self.windows.push(win);
                        }
                    }
                }
            }
            ipc::MSG_KEY_PRESSED => {
                // Route keyboard input to the currently focused (top Z-order) window
                if let Some(active_win) = self.windows.last() {
                    let mut fwd = ipc::Message::empty();
                    fwd.msg_type = ipc::MSG_KEY_PRESSED;
                    fwd.arg1 = msg.arg1;
                    let _ = ipc::send(active_win.owner_pid, &fwd);
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
                    let total_w = width + 2 * BORDER_WIDTH;
                    let total_h = height + TITLE_BAR_HEIGHT + 2 * BORDER_WIDTH;
                    let center_x = self.width.saturating_sub(total_w) / 2;
                    let center_y = TOPBAR_HEIGHT + (self.height.saturating_sub(TOPBAR_HEIGHT).saturating_sub(total_h)) / 2;

                    let mut title = [0u8; 32];
                    let default_title = b"MONOLITH//TERM";
                    let len = default_title.len().min(32);
                    title[..len].copy_from_slice(&default_title[..len]);

                    let fx = (center_x + self.windows.len() * 24).min(self.width.saturating_sub(total_w + 8));
                    let fy = (center_y + self.windows.len() * 24).min(self.height.saturating_sub(total_h + 8));

                    let win = Window {
                        id: self.next_window_id,
                        owner_pid,
                        x: fx,
                        y: fy,
                        float_x: fx,
                        float_y: fy,
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
                    if self.tile_mode {
                        self.apply_layout();
                    }
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

// ─── Full 6x8 Crisp Bitmap Font (Uppercase, Lowercase, Digits & Symbols) ───

fn get_font_glyph(ch: u8) -> [u8; 8] {
    match ch {
        // Uppercase
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

        // Distinct Lowercase (a..z)
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

        // Digits
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

        // Symbols & Punctuation
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
    let mut comp = Compositor::new();
    comp.init();
    comp.render();

    loop {
        let msg = ipc::receive();
        comp.handle_message(&msg);

        while let Some(msg) = ipc::try_receive() {
            comp.handle_message(&msg);
        }

        if comp.dirty {
            comp.render();
        }
    }
}
