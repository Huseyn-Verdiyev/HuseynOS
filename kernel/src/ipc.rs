use crate::process::Pid;
use crate::scheduler;
use crate::{serial_print, serial_println};
use spin::Mutex;

/// Max messages per process queue.
const QUEUE_SIZE: usize = 16;
/// Max payload size per message.
pub const MSG_PAYLOAD_SIZE: usize = 64;

/// An IPC message.
#[derive(Clone)]
pub struct Message {
    pub sender: Pid,
    pub payload: [u8; MSG_PAYLOAD_SIZE],
    pub len: usize,
}

impl Message {
    pub fn new(sender: Pid, data: &[u8]) -> Self {
        let mut payload = [0u8; MSG_PAYLOAD_SIZE];
        let len = data.len().min(MSG_PAYLOAD_SIZE);
        payload[..len].copy_from_slice(&data[..len]);
        Self { sender, payload, len }
    }

    pub fn as_str(&self) -> &str {
        core::str::from_utf8(&self.payload[..self.len]).unwrap_or("<invalid utf8>")
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
pub fn send(to_pid: Pid, data: &[u8]) -> bool {
    let from_pid = scheduler::current_pid();
    let msg = Message::new(from_pid, data);

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

/// Receive a message (blocking). Blocks until a message is available.
pub fn receive() -> Message {
    loop {
        if let Some(msg) = try_receive() {
            return msg;
        }
        // No message — yield (in a real system we'd block)
        scheduler::yield_now();
    }
}
