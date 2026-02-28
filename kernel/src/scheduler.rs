use core::arch::asm;
use crate::process::{self, Pid, ProcessState, InterruptContext};
use crate::{serial_print, serial_println};

/// Current running process PID. 0 = idle/kernel main.
static mut CURRENT_PID: Pid = 0;

/// Tick counter for preemptive scheduling.
static mut TICK_COUNT: u64 = 0;

/// Idle context (kernel main's RSP).
static mut IDLE_CTX_PTR: u64 = 0;

/// Initialize the scheduler.
pub fn init() {
    serial_println!("[OK] Scheduler initialized");
}

/// Get the current running PID.
pub fn current_pid() -> Pid {
    unsafe { CURRENT_PID }
}

/// Called from the timer IRQ handler or Syscall. Returns the next context.
pub fn schedule(current_context_ptr: *mut InterruptContext) -> *mut InterruptContext {
    unsafe {
        TICK_COUNT += 1;
        let old_pid = CURRENT_PID;

        // Save current context
        if old_pid != 0 {
            if let Some(old) = process::get_process_mut(old_pid) {
                old.context.rsp = current_context_ptr as u64;
                if old.state == ProcessState::Running {
                    old.state = ProcessState::Ready;
                }
            }
        } else {
            IDLE_CTX_PTR = current_context_ptr as u64;
        }

        // Find next ready process
        let next_pid = match process::next_ready(old_pid) {
            Some(pid) => pid,
            None => {
                // If no ready processes, switch to idle
                if old_pid == 0 {
                    return current_context_ptr;
                } else {
                    CURRENT_PID = 0;
                    return IDLE_CTX_PTR as *mut InterruptContext;
                }
            }
        };

        if let Some(new) = process::get_process_mut(next_pid) {
            new.state = ProcessState::Running;
            CURRENT_PID = next_pid;
            new.context.rsp as *mut InterruptContext
        } else {
            CURRENT_PID = 0;
            IDLE_CTX_PTR as *mut InterruptContext
        }
    }
}

/// Yield the current process's timeslice via Syscall.
pub fn yield_now() {
    unsafe {
        asm!("int 0x80", in("rax") 4u64, options(nomem, nostack)); // 4 = SYS_YIELD
    }
}

/// Block the current process.
pub fn block_current() {
    unsafe {
        if CURRENT_PID != 0 {
            if let Some(p) = process::get_process_mut(CURRENT_PID) {
                p.state = ProcessState::Blocked;
            }
        }
    }
    yield_now();
}

/// Unblock a process by PID.
pub fn unblock(pid: Pid) {
    if let Some(p) = process::get_process_mut(pid) {
        if p.state == ProcessState::Blocked {
            p.state = ProcessState::Ready;
        }
    }
}
