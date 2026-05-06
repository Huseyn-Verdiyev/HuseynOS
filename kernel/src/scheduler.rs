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

        let next_ctx_ptr = if let Some(new) = process::get_process_mut(next_pid) {
            new.state = ProcessState::Running;
            CURRENT_PID = next_pid;

            // Page Table Swapping
            let pml4 = if new.pml4_phys != 0 {
                new.pml4_phys
            } else {
                crate::memory::paging::KERNEL_PML4_PHYS
            };
            let current_cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
            if (current_cr3 & !0xFFF) != pml4 {
                core::arch::asm!("mov cr3, {}", in(reg) pml4);
            }

            // Update TSS RSP0 to this thread's kernel stack
            crate::gdt::set_tss_stack(new.kernel_stack_top);

            new.context.rsp as *mut InterruptContext
        } else {
            CURRENT_PID = 0;
            let pml4 = crate::memory::paging::KERNEL_PML4_PHYS;
            let current_cr3: u64;
            core::arch::asm!("mov {}, cr3", out(reg) current_cr3);
            if (current_cr3 & !0xFFF) != pml4 {
                core::arch::asm!("mov cr3, {}", in(reg) pml4);
            }
            
            // For idle task, we can just use the saved idle pointer + size of context
            // though the idle task never runs in Ring 3 so it technically doesn't matter.
            crate::gdt::set_tss_stack(IDLE_CTX_PTR + core::mem::size_of::<InterruptContext>() as u64);
            
            IDLE_CTX_PTR as *mut InterruptContext
        };

        next_ctx_ptr
    }
}

/// Yield the current process's timeslice via Syscall.
pub fn yield_now() {
    unsafe {
        asm!("int 0x80", in("rax") 4u64, options(nomem, nostack)); // 4 = SYS_YIELD
    }
}

/// Block the current process without forcing an interrupt
pub fn block_current() {
    unsafe {
        if CURRENT_PID != 0 {
            if let Some(p) = process::get_process_mut(CURRENT_PID) {
                p.state = ProcessState::Blocked;
            }
        }
    }
}

/// Unblock a process by PID.
pub fn unblock(pid: Pid) {
    if let Some(p) = process::get_process_mut(pid) {
        if p.state == ProcessState::Blocked {
            p.state = ProcessState::Ready;
        }
    }
}
