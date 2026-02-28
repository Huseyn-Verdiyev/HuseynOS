use alloc::boxed::Box;

/// Maximum number of processes.
pub const MAX_PROCESSES: usize = 64;

/// Size of each kernel stack (16 KiB).
pub const KERNEL_STACK_SIZE: usize = 16384;

/// Process ID type.
pub type Pid = usize;

/// Process state.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ProcessState {
    Empty,
    Ready,
    Running,
    Blocked,
    Dead,
}

/// CPU Interrupt Context (Pushed by HW and isr_common)
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct InterruptContext {
    pub r15: u64, pub r14: u64, pub r13: u64, pub r12: u64, pub r11: u64, pub r10: u64, pub r9: u64, pub r8: u64,
    pub rdi: u64, pub rsi: u64, pub rbp: u64, pub rbx: u64, pub rdx: u64, pub rcx: u64, pub rax: u64,
    pub int_num: u64,
    pub error_code: u64,
    pub rip: u64,
    pub cs: u64,
    pub rflags: u64,
    pub rsp: u64,
    pub ss: u64,
}

/// Saved CPU context for context switching.
#[derive(Debug, Clone, Copy)]
pub struct Context {
    /// Saved stack pointer — points to an InterruptContext
    pub rsp: u64,
}

impl Context {
    pub const fn empty() -> Self {
        Self { rsp: 0 }
    }
}

/// A kernel process/task.
pub struct Process {
    pub pid: Pid,
    pub state: ProcessState,
    pub context: Context,
    pub stack: Option<Box<[u8; KERNEL_STACK_SIZE]>>,
    pub name: &'static str,
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcessState::Empty,
            context: Context::empty(),
            stack: None,
            name: "",
        }
    }
}

/// Global process table.
static mut PROCESS_TABLE: [Option<Process>; MAX_PROCESSES] = {
    const NONE: Option<Process> = None;
    [NONE; MAX_PROCESSES]
};

static mut NEXT_PID: Pid = 1;

/// Entry trampoline — called by a new task, wraps the actual entry fn.
/// This ensures proper stack alignment and calls the real function.
fn task_entry_trampoline() {
    // The real entry fn pointer is stored in r12 by spawn()
    let entry_fn: fn();
    unsafe {
        core::arch::asm!("mov {}, r12", out(reg) entry_fn);
    }
    entry_fn();
    // If task returns, mark it as Dead and yield
    crate::process::exit_process(crate::scheduler::current_pid());
    loop { crate::scheduler::yield_now(); }
}

/// Spawn a new kernel task. `entry` is the function to run.
pub fn spawn(name: &'static str, entry: fn()) -> Pid {
    unsafe {
        let pid = NEXT_PID;
        NEXT_PID += 1;

        // Allocate kernel stack
        let stack = Box::new([0u8; KERNEL_STACK_SIZE]);
        let stack_base = stack.as_ptr() as u64;
        let mut stack_top = stack_base + KERNEL_STACK_SIZE as u64;

        // Align stack to 16 bytes
        stack_top &= !0xF;

        // Allocate space for InterruptContext
        let context_size = core::mem::size_of::<InterruptContext>() as u64;
        let context_ptr = (stack_top - context_size) as *mut InterruptContext;
        
        // Zero it out
        core::ptr::write_bytes(context_ptr, 0, 1);
        
        let ctx_obj = &mut *context_ptr;
        ctx_obj.ss = 0x10; // Kernel Data Segment
        ctx_obj.rsp = stack_top;
        ctx_obj.rflags = 0x202; // IF enabled
        ctx_obj.cs = 0x08; // Kernel Code Segment
        ctx_obj.rip = task_entry_trampoline as u64;
        
        // Passing the entry function pointer to the trampoline via r12
        ctx_obj.r12 = entry as u64;
        
        // The context pointer that isr_common will restore from
        let ctx = Context {
            rsp: context_ptr as u64,
        };

        let proc = Process {
            pid,
            state: ProcessState::Ready,
            context: ctx,
            stack: Some(stack),
            name,
        };

        // Find empty slot
        for slot in PROCESS_TABLE.iter_mut() {
            if slot.is_none() {
                *slot = Some(proc);
                return pid;
            }
        }

        panic!("Process table full!");
    }
}

/// Get a mutable reference to a process by PID.
pub fn get_process_mut(pid: Pid) -> Option<&'static mut Process> {
    unsafe {
        for slot in PROCESS_TABLE.iter_mut() {
            if let Some(ref mut p) = slot {
                if p.pid == pid {
                    return Some(p);
                }
            }
        }
        None
    }
}

/// Get the next Ready process PID after `current_pid` (round-robin).
pub fn next_ready(current_pid: Pid) -> Option<Pid> {
    unsafe {
        let len = MAX_PROCESSES;
        let mut start = 0;
        for (i, slot) in PROCESS_TABLE.iter().enumerate() {
            if let Some(ref p) = slot {
                if p.pid == current_pid {
                    start = i + 1;
                    break;
                }
            }
        }

        for offset in 0..len {
            let idx = (start + offset) % len;
            if let Some(ref p) = PROCESS_TABLE[idx] {
                if p.state == ProcessState::Ready {
                    return Some(p.pid);
                }
            }
        }
        None
    }
}

/// Mark a process as Dead.
pub fn exit_process(pid: Pid) {
    if let Some(p) = get_process_mut(pid) {
        p.state = ProcessState::Dead;
    }
}

/// Count of alive (Ready/Running/Blocked) processes.
pub fn alive_count() -> usize {
    unsafe {
        PROCESS_TABLE.iter()
            .filter(|s| s.as_ref().map_or(false, |p| {
                p.state == ProcessState::Ready
                    || p.state == ProcessState::Running
                    || p.state == ProcessState::Blocked
            }))
            .count()
    }
}
