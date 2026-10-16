#![no_std]
#![no_main]

use libhuseyn::ipc;

/// PS/2 & USB-HID mouse state with dynamic resolution bounds (1080p/2K/4K ready)
/// and ballistic acceleration for high-DPI displays.
struct MouseState {
    phase: u8,
    packet: [u8; 3],
    x: i32,
    y: i32,
    buttons: u8,
    screen_w: i32,
    screen_h: i32,
}

impl MouseState {
    fn new(w: i32, h: i32) -> Self {
        let safe_w = if w > 0 { w } else { 1024 };
        let safe_h = if h > 0 { h } else { 768 };
        Self {
            phase: 0,
            packet: [0; 3],
            x: safe_w / 2,
            y: safe_h / 2,
            buttons: 0,
            screen_w: safe_w,
            screen_h: safe_h,
        }
    }

    /// Process one byte from the mouse stream. Returns true when a full 3-byte packet is ready.
    fn feed(&mut self, byte: u8) -> bool {
        match self.phase {
            0 => {
                if byte & 0x08 != 0 {
                    self.packet[0] = byte;
                    self.phase = 1;
                }
                false
            }
            1 => {
                self.packet[1] = byte;
                self.phase = 2;
                false
            }
            2 => {
                self.packet[2] = byte;
                self.phase = 0;
                self.process_packet();
                true
            }
            _ => {
                self.phase = 0;
                false
            }
        }
    }

    fn process_packet(&mut self) {
        let flags = self.packet[0];
        if flags & 0xC0 != 0 {
            return; // Discard X/Y overflow packets
        }

        self.buttons = flags & 0x07;

        let mut dx = self.packet[1] as i32;
        if flags & 0x10 != 0 {
            dx -= 256;
        }

        let mut dy = self.packet[2] as i32;
        if flags & 0x20 != 0 {
            dy -= 256;
        }
        dy = -dy;

        // Subtle ballistic acceleration for 1080p / 4K displays
        if dx.abs() > 8 {
            dx = dx * 3 / 2;
        }
        if dy.abs() > 8 {
            dy = dy * 3 / 2;
        }

        self.x = (self.x + dx).clamp(0, self.screen_w - 1);
        self.y = (self.y + dy).clamp(0, self.screen_h - 1);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let (_, w, h, _, _) = libhuseyn::get_fb_info();
    let mut state = MouseState::new(w as i32, h as i32);
    let mut prev_buttons: u8 = 0;

    loop {
        let msg = ipc::receive();

        if msg.msg_type == ipc::MSG_MOUSE_PACKET {
            let byte = msg.arg1 as u8;

            if state.feed(byte) {
                let mut move_msg = ipc::Message::empty();
                move_msg.msg_type = ipc::MSG_MOUSE_MOVE;
                move_msg.arg1 = state.x as u64;
                move_msg.arg2 = state.y as u64;
                move_msg.arg3 = state.buttons as u64;
                while !ipc::send(5, &move_msg) {
                    libhuseyn::yield_now();
                }

                if state.buttons != prev_buttons {
                    let mut click_msg = ipc::Message::empty();
                    click_msg.msg_type = ipc::MSG_MOUSE_CLICK;
                    click_msg.arg1 = state.x as u64;
                    click_msg.arg2 = state.y as u64;
                    click_msg.arg3 = state.buttons as u64;
                    while !ipc::send(5, &click_msg) {
                        libhuseyn::yield_now();
                    }
                    prev_buttons = state.buttons;
                }
            }
        }
    }
}
