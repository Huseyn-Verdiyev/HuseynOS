#![no_std]

use core::arch::asm;
use core::panic::PanicInfo;

pub mod ipc;
pub mod console;
pub mod alloc;

/// Available System Calls
pub const SYS_SEND: u64 = 1;
pub const SYS_RECV: u64 = 2;
pub const SYS_EXIT: u64 = 3;
pub const SYS_YIELD: u64 = 4;
pub const SYS_GETPID: u64 = 5;
pub const SYS_MAP_PHYSICAL: u64 = 6;
pub const SYS_INB: u64 = 7;
pub const SYS_GET_FB_INFO: u64 = 8;
pub const SYS_MMAP: u64 = 9;
pub const SYS_MUNMAP: u64 = 10;
pub const SYS_SBRK: u64 = 11;
pub const SYS_SHM_CREATE: u64 = 12;
pub const SYS_SHM_MAP: u64 = 13;
pub const SYS_SHM_UNMAP: u64 = 14;
pub const SYS_SPAWN: u64 = 15;
pub const SYS_EXECVE: u64 = 16;
pub const SYS_WAITPID: u64 = 17;
pub const SYS_OPEN: u64 = 18;
pub const SYS_READ: u64 = 19;
pub const SYS_WRITE_FILE: u64 = 20;
pub const SYS_CLOSE: u64 = 21;
pub const SYS_STAT: u64 = 22;
pub const SYS_LISTDIR: u64 = 23;
pub const SYS_SHUTDOWN: u64 = 24;
pub const SYS_GET_TIME: u64 = 25;

/// Raw syscall invocation with up to 6 arguments
#[inline]
pub unsafe fn syscall(num: u64, arg1: u64, arg2: u64, arg3: u64, arg4: u64, arg5: u64, arg6: u64) -> u64 {
    let mut ret: u64;
    unsafe {
        asm!(
            "int 0x80",
            inout("rax") num => ret,
            in("rdi") arg1,
            in("rsi") arg2,
            in("rdx") arg3,
            in("r10") arg4,
            in("r8") arg5,
            in("r9") arg6,
            options(nostack, preserves_flags)
        );
    }
    ret
}

/// Yield the CPU to the next ready process
pub fn yield_now() {
    unsafe {
        syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0);
    }
}

/// Exit the current process natively
pub fn exit(code: u64) -> ! {
    unsafe {
        syscall(SYS_EXIT, code, 0, 0, 0, 0, 0);
    }
    loop {}
}

/// Get the current process ID
pub fn getpid() -> usize {
    unsafe {
        syscall(SYS_GETPID, 0, 0, 0, 0, 0, 0) as usize
    }
}

#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    exit(1);
}

/// Map physical memory to a virtual address.
pub fn map_physical(virt_addr: u64, phys_addr: u64, size: u64) -> Result<(), ()> {
    unsafe {
        let res = syscall(SYS_MAP_PHYSICAL, virt_addr, phys_addr, size, 0, 0, 0);
        if res == 0 {
            Ok(())
        } else {
            Err(())
        }
    }
}

/// Read a byte from an I/O port.
pub fn inb(port: u16) -> u8 {
    unsafe {
        syscall(SYS_INB, port as u64, 0, 0, 0, 0, 0) as u8
    }
}

/// Extend the process heap. Returns the OLD break address.
/// `increment` is in bytes.
pub fn sbrk(increment: usize) -> *mut u8 {
    unsafe {
        syscall(SYS_SBRK, increment as u64, 0, 0, 0, 0, 0) as *mut u8
    }
}

/// Map anonymous memory. Returns the mapped address.
pub fn mmap(hint: u64, size: usize) -> *mut u8 {
    unsafe {
        let addr = syscall(SYS_MMAP, hint, size as u64, 0, 0, 0, 0);
        if addr == u64::MAX { core::ptr::null_mut() } else { addr as *mut u8 }
    }
}

/// Unmap memory previously mapped with mmap.
pub fn munmap(addr: *mut u8, size: usize) -> i32 {
    unsafe {
        syscall(SYS_MUNMAP, addr as u64, size as u64, 0, 0, 0, 0) as i32
    }
}

/// Retrieve Limine Framebuffer Info from the Kernel
pub fn get_fb_info() -> (u64, u64, u64, u64, u8) {
    let mut paddr: u64;
    let mut width: u64;
    let mut height: u64;
    let mut pitch: u64;
    let mut bpp: u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inout("rax") SYS_GET_FB_INFO => paddr,
            out("rdi") width,
            out("rsi") height,
            out("rdx") pitch,
            out("r10") bpp,
            options(nostack, preserves_flags)
        );
    }
    (paddr, width, height, pitch, bpp as u8)
}

/// Create a new shared memory region. Returns the SHM ID.
pub fn shm_create(size: usize) -> u32 {
    unsafe {
        syscall(SYS_SHM_CREATE, size as u64, 0, 0, 0, 0, 0) as u32
    }
}

/// Map a shared memory region into the current process's address space.
pub fn shm_map(shm_id: u32, virt_addr: u64) -> Result<usize, ()> {
    unsafe {
        let result = syscall(SYS_SHM_MAP, shm_id as u64, virt_addr, 0, 0, 0, 0);
        if result == u64::MAX { Err(()) } else { Ok(result as usize) }
    }
}

/// Unmap a shared memory region.
pub fn shm_unmap(shm_id: u32, virt_addr: u64) -> Result<(), ()> {
    unsafe {
        let result = syscall(SYS_SHM_UNMAP, shm_id as u64, virt_addr, 0, 0, 0, 0);
        if result == 0 { Ok(()) } else { Err(()) }
    }
}

/// Spawn a new process from an ELF file on disk. Returns the new PID or an error.
pub fn spawn(filename: &str) -> Result<u64, ()> {
    unsafe {
        let result = syscall(
            SYS_SPAWN,
            filename.as_ptr() as u64,
            filename.len() as u64,
            0,
            0,
            0,
            0,
        );
        if result == u64::MAX {
            Err(())
        } else {
            Ok(result)
        }
    }
}

/// Replace the current process with a new ELF program.
pub fn execve(filename: &str) -> Result<(), ()> {
    unsafe {
        let result = syscall(
            SYS_EXECVE,
            filename.as_ptr() as u64,
            filename.len() as u64,
            0,
            0,
            0,
            0,
        );
        if result == u64::MAX {
            Err(())
        } else {
            Ok(())
        }
    }
}

/// Wait for a child process to exit and return its exit code.
pub fn waitpid(pid: u64) -> Result<u64, ()> {
    unsafe {
        let result = syscall(SYS_WAITPID, pid, 0, 0, 0, 0, 0);
        if result == u64::MAX {
            Err(())
        } else {
            Ok(result)
        }
    }
}

// ─── File I/O ───

/// Open a file. flags: 0=read-only, 1=write. Returns fd or error.
pub fn open(filename: &str, flags: u64) -> Result<u64, ()> {
    unsafe {
        let result = syscall(SYS_OPEN, filename.as_ptr() as u64, filename.len() as u64, flags, 0, 0, 0);
        if result == u64::MAX { Err(()) } else { Ok(result) }
    }
}

/// Read up to `count` bytes from fd into buf. Returns bytes read.
pub fn read(fd: u64, buf: &mut [u8]) -> usize {
    unsafe {
        syscall(SYS_READ, fd, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0) as usize
    }
}

/// Write bytes from buf to fd. Returns bytes written.
pub fn write_file(fd: u64, buf: &[u8]) -> usize {
    unsafe {
        syscall(SYS_WRITE_FILE, fd, buf.as_ptr() as u64, buf.len() as u64, 0, 0, 0) as usize
    }
}

/// Close a file descriptor.
pub fn close(fd: u64) -> bool {
    unsafe { syscall(SYS_CLOSE, fd, 0, 0, 0, 0, 0) == 0 }
}

/// Get file size via fd.
pub fn stat(fd: u64) -> u64 {
    unsafe { syscall(SYS_STAT, fd, 0, 0, 0, 0, 0) }
}

/// List directory contents into buf. Returns bytes written.
pub fn listdir(buf: &mut [u8]) -> usize {
    unsafe {
        syscall(SYS_LISTDIR, buf.as_mut_ptr() as u64, buf.len() as u64, 0, 0, 0, 0) as usize
    }
}

// ─── C-like String Utilities ───

/// Returns the length of a null-terminated C string.
pub fn strlen(s: *const u8) -> usize {
    let mut len = 0;
    unsafe {
        while *s.add(len) != 0 {
            len += 1;
        }
    }
    len
}

/// Compare two null-terminated C strings. Returns 0 if equal.
pub fn strcmp(a: *const u8, b: *const u8) -> i32 {
    let mut i = 0;
    unsafe {
        loop {
            let ca = *a.add(i);
            let cb = *b.add(i);
            if ca != cb {
                return ca as i32 - cb as i32;
            }
            if ca == 0 {
                return 0;
            }
            i += 1;
        }
    }
}

/// Copy `n` bytes from `src` to `dst`.
pub fn memcpy(dst: *mut u8, src: *const u8, n: usize) {
    unsafe {
        core::ptr::copy_nonoverlapping(src, dst, n);
    }
}

/// Fill `n` bytes at `dst` with `val`.
pub fn memset(dst: *mut u8, val: u8, n: usize) {
    unsafe {
        core::ptr::write_bytes(dst, val, n);
    }
}

// ─── C-like Memory Allocation ───

use core::alloc::Layout;

/// Allocate `size` bytes of memory (like C's malloc).
pub fn malloc(size: usize) -> *mut u8 {
    if size == 0 {
        return core::ptr::null_mut();
    }
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, 8);
        extern crate alloc as alloc_crate;
        alloc_crate::alloc::alloc(layout)
    }
}

/// Free memory previously allocated by `malloc`.
pub fn free(ptr: *mut u8, size: usize) {
    if ptr.is_null() || size == 0 {
        return;
    }
    unsafe {
        let layout = Layout::from_size_align_unchecked(size, 8);
        extern crate alloc as alloc_crate;
        alloc_crate::alloc::dealloc(ptr, layout);
    }
}

/// Reallocate memory (like C's realloc).
pub fn realloc(ptr: *mut u8, old_size: usize, new_size: usize) -> *mut u8 {
    if ptr.is_null() {
        return malloc(new_size);
    }
    if new_size == 0 {
        free(ptr, old_size);
        return core::ptr::null_mut();
    }
    unsafe {
        let layout = Layout::from_size_align_unchecked(old_size, 8);
        extern crate alloc as alloc_crate;
        alloc_crate::alloc::realloc(ptr, layout, new_size)
    }
}

// ─── DateTime ───

/// Date and time structure returned by the RTC syscall
#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    pub hour: u8,
    pub minute: u8,
    pub second: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
}

/// Get the current date and time from the kernel RTC.
pub fn get_time() -> DateTime {
    let hour: u64;
    let minute: u64;
    let second: u64;
    let day: u64;
    let month: u64;
    let year: u64;
    unsafe {
        core::arch::asm!(
            "int 0x80",
            inout("rax") SYS_GET_TIME => hour,
            out("rdi") minute,
            out("rsi") second,
            out("rdx") day,
            out("r10") month,
            out("r8") year,
            options(nostack, preserves_flags)
        );
    }
    DateTime {
        hour: hour as u8,
        minute: minute as u8,
        second: second as u8,
        day: day as u8,
        month: month as u8,
        year: year as u16,
    }
}

/// Shutdown the system via ACPI.
pub fn shutdown() -> ! {
    unsafe {
        syscall(SYS_SHUTDOWN, 0, 0, 0, 0, 0, 0);
    }
    loop {}
}
