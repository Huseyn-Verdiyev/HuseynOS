#![no_std]
#![no_main]

use libhuseyn::ipc;

const KEYMAP_NORMAL: [u8; 128] = [
    0, 27, b'1', b'2', b'3', b'4', b'5', b'6', b'7', b'8', b'9', b'0', b'-', b'=', b'\x08', // 0x00 - 0x0E (0x08 = Backspace)
    b'\t', b'q', b'w', b'e', b'r', b't', b'y', b'u', b'i', b'o', b'p', b'[', b']', b'\n',    // 0x0F - 0x1C (0x1C = Enter)
    0, b'a', b's', b'd', b'f', b'g', b'h', b'j', b'k', b'l', b';', b'\'', b'`',             // 0x1D - 0x29 (0x1D = LCtrl)
    0, b'\\', b'z', b'x', b'c', b'v', b'b', b'n', b'm', b',', b'.', b'/', 0, b'*',          // 0x2A - 0x37 (0x2A = LShift)
    0, b' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,                                      // 0x38 - 0x47 (0x39 = Space)
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Add 8 more to make 128
    0, 0, 0, 0, 0, 0, 0, 0,
];

const KEYMAP_SHIFT: [u8; 128] = [
    0, 27, b'!', b'@', b'#', b'$', b'%', b'^', b'&', b'*', b'(', b')', b'_', b'+', b'\x08',
    b'\t', b'Q', b'W', b'E', b'R', b'T', b'Y', b'U', b'I', b'O', b'P', b'{', b'}', b'\n',
    0, b'A', b'S', b'D', b'F', b'G', b'H', b'J', b'K', b'L', b':', b'"', b'~',
    0, b'|', b'Z', b'X', b'C', b'V', b'B', b'N', b'M', b'<', b'>', b'?', 0, b'*',
    0, b' ', 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0,
    // Add 8 more to make 128
    0, 0, 0, 0, 0, 0, 0, 0,
];

struct ModifierState {
    lshift: bool,
    rshift: bool,
    capslock: bool,
}

impl ModifierState {
    const fn new() -> Self {
        Self {
            lshift: false,
            rshift: false,
            capslock: false,
        }
    }

    fn is_shifted(&self) -> bool {
        self.lshift || self.rshift
    }
}

fn translate_scancode(scancode: u8, state: &ModifierState) -> Option<u8> {
    if scancode >= 128 {
        return None;
    }

    let ascii = if state.is_shifted() {
        KEYMAP_SHIFT[scancode as usize]
    } else {
        KEYMAP_NORMAL[scancode as usize]
    };

    if ascii == 0 {
        return None;
    }

    // Apply capslock (only to letters)
    let mut mapped = ascii;
    if state.capslock && mapped.is_ascii_alphabetic() {
        if mapped.is_ascii_lowercase() && !state.is_shifted() {
            mapped = mapped.to_ascii_uppercase();
        } else if mapped.is_ascii_uppercase() && state.is_shifted() {
            mapped = mapped.to_ascii_lowercase();
        }
    }

    Some(mapped)
}

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let mut state = ModifierState::new();

    loop {
        let msg = ipc::receive();
        
        // MSG_HARDWARE_INTERRUPT implies we got a scancode
        if msg.msg_type == ipc::MSG_HARDWARE_INTERRUPT {
            let scancode = msg.arg1 as u8;

            // Check for key release (make/break code)
            if scancode >= 0x80 {
                match scancode - 0x80 {
                    0x2A => state.lshift = false,
                    0x36 => state.rshift = false,
                    _ => {}
                }
                continue;
            }

            // Key presses
            match scancode {
                0x2A => state.lshift = true,
                0x36 => state.rshift = true,
                0x3A => state.capslock = !state.capslock,
                _ => {
                    if let Some(ascii) = translate_scancode(scancode, &state) {
                        // Forward ASCII char to GUI Terminal (PID 6)
                        let mut out_msg = ipc::Message::empty();
                        out_msg.msg_type = ipc::MSG_KEY_PRESSED;
                        out_msg.arg1 = ascii as u64;
                        
                        // Keep trying until mailbox isn't full
                        while !ipc::send(6, &out_msg) {
                            libhuseyn::yield_now();
                        }
                    }
                }
            }
        }
    }
}

