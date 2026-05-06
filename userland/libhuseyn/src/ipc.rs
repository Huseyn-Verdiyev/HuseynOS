use crate::{syscall, SYS_SEND, SYS_RECV};

pub const MSG_PRINT_CHAR: u64 = 0x10;
pub const MSG_KEY_PRESSED: u64 = 0x20;
pub const MSG_HARDWARE_INTERRUPT: u64 = 0x30;
pub const MSG_MOUSE_PACKET: u64 = 0x40;
pub const MSG_MOUSE_MOVE: u64 = 0x41;
pub const MSG_MOUSE_CLICK: u64 = 0x42;
pub const MSG_WINDOW_CREATE: u64 = 0x50; // arg1=width, arg2=height, arg3=shm_id
pub const MSG_WINDOW_DAMAGE: u64 = 0x51; // Window content updated
pub const MSG_QUIT: u64 = 0x60;           // Request process to exit gracefully
pub const MSG_PING: u64 = 0xAA;
pub const MSG_PONG: u64 = 0xBB;
/// An IPC message.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Message {
    pub sender: usize,
    pub msg_type: u64,
    pub arg1: u64,
    pub arg2: u64,
    pub arg3: u64,
    pub arg4: u64,
    pub arg5: u64,
    pub arg6: u64,
}

impl Message {
    pub const fn empty() -> Self {
        Self {
            sender: 0,
            msg_type: 0,
            arg1: 0,
            arg2: 0,
            arg3: 0,
            arg4: 0,
            arg5: 0,
            arg6: 0,
        }
    }
}

/// Send a message to the target PID
pub fn send(to_pid: usize, msg: &Message) -> bool {
    unsafe {
        let res = syscall(
            SYS_SEND,
            to_pid as u64,
            msg.msg_type,
            msg.arg1,
            msg.arg2,
            msg.arg3,
            msg.arg4,
        );
        // The core syscall currently only takes 7 args (num + 6 args).
        // arg5 and arg6 are truncated in the basic system call interface.
        res == 0
    }
}

/// Receive a message. Blocks until a message arrives.
pub fn receive() -> Message {
    let mut msg = Message::empty();
    let msg_ptr = &mut msg as *mut Message as u64;
    
    unsafe {
        let _sender = syscall(SYS_RECV, msg_ptr, 0, 0, 0, 0, 0);
        msg
    }
}

/// Try to receive a message without blocking. Returns None if queue is empty.
pub fn try_receive() -> Option<Message> {
    let mut msg = Message::empty();
    let msg_ptr = &mut msg as *mut Message as u64;
    
    unsafe {
        let result = syscall(SYS_RECV, msg_ptr, 1, 0, 0, 0, 0); // arg2=1 means non-blocking
        if result == u64::MAX {
            None
        } else {
            Some(msg)
        }
    }
}
