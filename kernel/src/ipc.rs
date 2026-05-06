use crate::process::Pid;
use crate::scheduler;
use crate::{serial_print, serial_println};
use spin::Mutex;

/// Max messages per process queue.
const QUEUE_SIZE: usize = 16;
/// Max payload size per message.
pub const MSG_PAYLOAD_SIZE: usize = 64;

/// An IPC message.
#[derive(Clone, Copy, Debug)]
#[repr(C)]
pub struct Message {
    pub sender: Pid,
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

/// Per-process message queue (ring buffer).
struct MessageQueue {
    messages: [Option<Message>; QUEUE_SIZE],
    head: usize,
    tail: usize,
    count: usize,
}

impl MessageQueue {
    const fn new() -> Self {
        const NONE: Option<Message> = None;
        Self {
            messages: [NONE; QUEUE_SIZE],
            head: 0,
            tail: 0,
            count: 0,
        }
    }

    fn push(&mut self, msg: Message) -> bool {
        if self.count >= QUEUE_SIZE {
            return false;
        }
        self.messages[self.tail] = Some(msg);
        self.tail = (self.tail + 1) % QUEUE_SIZE;
        self.count += 1;
        true
    }

    fn pop(&mut self) -> Option<Message> {
        if self.count == 0 {
            return None;
        }
        let msg = self.messages[self.head].take();
        self.head = (self.head + 1) % QUEUE_SIZE;
        self.count -= 1;
        msg
    }

    fn is_empty(&self) -> bool {
        self.count == 0
    }
}

/// Global IPC queues — one per PID slot.
static IPC_QUEUES: Mutex<[MessageQueue; 64]> = {
    const Q: MessageQueue = MessageQueue::new();
    Mutex::new([Q; 64])
};

/// Send a message to a target process.
pub fn send(to_pid: Pid, msg_type: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) -> bool {
    let from_pid = scheduler::current_pid();
    let msg = Message {
        sender: from_pid,
        msg_type,
        arg1,
        arg2,
        arg3,
        arg4,
        arg5,
        arg6,
    };

    let mut queues = IPC_QUEUES.lock();
    if to_pid == 0 || to_pid >= 64 {
        return false;
    }
    let ok = queues[to_pid].push(msg);

    if ok {
        // Unblock receiver if it was waiting
        drop(queues); // Release lock before scheduler call
        scheduler::unblock(to_pid);
    }
    ok
}

/// Receive a message (non-blocking). Returns None if queue empty.
pub fn try_receive() -> Option<Message> {
    let pid = scheduler::current_pid();
    let mut queues = IPC_QUEUES.lock();
    if pid < 64 {
        queues[pid].pop()
    } else {
        None
    }
}

