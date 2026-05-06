#![no_std]
#![no_main]

use libhuseyn::ipc;

const SCREEN_WIDTH: i32 = 1024;
const SCREEN_HEIGHT: i32 = 768;

/// PS/2 mouse sends 3-byte packets:
///   Byte 0: [YO|XO|YS|XS|1|MB|RB|LB]
///   Byte 1: X movement (signed, with sign in byte 0 bit 4)
///   Byte 2: Y movement (signed, with sign in byte 0 bit 5)
struct MouseState {
    phase: u8,       // 0, 1, or 2 — which byte of the 3-byte packet
    packet: [u8; 3], // current 3-byte packet being assembled
    x: i32,          // absolute cursor X
    y: i32,          // absolute cursor Y
    buttons: u8,     // button state (bit 0=left, bit 1=right, bit 2=middle)
}

impl MouseState {
    const fn new() -> Self {
        Self {
            phase: 0,
            packet: [0; 3],
            x: SCREEN_WIDTH / 2,
            y: SCREEN_HEIGHT / 2,
            buttons: 0,
        }
    }

    /// Process one byte from the PS/2 mouse. Returns true when a full packet is ready.
    fn feed(&mut self, byte: u8) -> bool {
        match self.phase {
            0 => {
                // Byte 0 must have bit 3 set (alignment bit)
                if byte & 0x08 != 0 {
                    self.packet[0] = byte;
                    self.phase = 1;
                }
                // If bit 3 is not set, discard (re-sync)
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
        self.buttons = flags & 0x07;

        // X movement — sign extend using bit 4 of flags
        let mut dx = self.packet[1] as i32;
        if flags & 0x10 != 0 {
            dx -= 256; // sign-extend: it's negative
        }

        // Y movement — sign extend using bit 5 of flags
        let mut dy = self.packet[2] as i32;
        if flags & 0x20 != 0 {
            dy -= 256;
        }

        // PS/2 Y is inverted (up = positive), so negate for screen coords
        dy = -dy;

        // Check for overflow flags and discard if set
        if flags & 0xC0 != 0 {
            return; // X or Y overflow, discard
        }

        self.x = (self.x + dx).clamp(0, SCREEN_WIDTH - 1);
        self.y = (self.y + dy).clamp(0, SCREEN_HEIGHT - 1);
    }
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut state = MouseState::new();
    let mut prev_buttons: u8 = 0;

    loop {
        let msg = ipc::receive();

        if msg.msg_type == ipc::MSG_MOUSE_PACKET {
            let byte = msg.arg1 as u8;

            if state.feed(byte) {
                // Full packet received - send mouse move to compositor (PID 5)
                let mut move_msg = ipc::Message::empty();
                move_msg.msg_type = ipc::MSG_MOUSE_MOVE;
                move_msg.arg1 = state.x as u64;
                move_msg.arg2 = state.y as u64;
                move_msg.arg3 = state.buttons as u64;
                while !ipc::send(5, &move_msg) {
                    libhuseyn::yield_now();
                }

                // Check for button state changes (clicks)
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
