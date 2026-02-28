use crate::ipc;
use crate::process;
use crate::scheduler;
use crate::{serial_print, serial_println};

/// Syscall numbers.
pub const SYS_SEND: u64 = 1;
pub const SYS_RECV: u64 = 2;
pub const SYS_EXIT: u64 = 3;
pub const SYS_YIELD: u64 = 4;
pub const SYS_GETPID: u64 = 5;

use crate::process::InterruptContext;

/// Handle a syscall. Called from the INT 0x80 handler.
/// Arguments: rax=syscall number, rdi=arg1, rsi=arg2, rdx=arg3
/// Returns the (potentially new) context pointer.
pub fn handle(context_ptr: *mut InterruptContext) -> *mut InterruptContext {
    let ctx = unsafe { &mut *context_ptr };
    let syscall_num = ctx.rax;
    let arg1 = ctx.rdi;
    let arg2 = ctx.rsi;
    let arg3 = ctx.rdx;

    match syscall_num {
        SYS_SEND => {
            // send(to_pid, data_ptr, data_len)
            let to_pid = arg1 as usize;
            let data_ptr = arg2 as *const u8;
            let data_len = (arg3 as usize).min(ipc::MSG_PAYLOAD_SIZE);
            let data = unsafe { core::slice::from_raw_parts(data_ptr, data_len) };
            ctx.rax = if ipc::send(to_pid, data) { 0 } else { 1 };
            context_ptr
        }
        SYS_RECV => {
            // receive() -> writes message to buffer at arg1, returns sender PID
            // Note: ipc::receive() internally calls scheduler::yield_now() if blocked,
            // which triggers another syscall. Here we just process it as is.
            let buf_ptr = arg1 as *mut u8;
            let buf_len = arg2 as usize;
            let msg = ipc::receive();
            let copy_len = msg.len.min(buf_len);
            unsafe {
                core::ptr::copy_nonoverlapping(msg.payload.as_ptr(), buf_ptr, copy_len);
            }
            ctx.rax = msg.sender as u64;
            context_ptr
        }
        SYS_EXIT => {
            process::exit_process(scheduler::current_pid());
            scheduler::schedule(context_ptr)
        }
        SYS_YIELD => {
            scheduler::schedule(context_ptr)
        }
        SYS_GETPID => {
            ctx.rax = scheduler::current_pid() as u64;
            context_ptr
        }
        _ => {
            serial_println!("[SYSCALL] Unknown syscall: {}", syscall_num);
            ctx.rax = u64::MAX;
            context_ptr
        }
    }
}
