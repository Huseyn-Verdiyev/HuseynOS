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

fn read_user_string(vaddr_start: u64, max_len: usize, pid: usize) -> Option<alloc::string::String> {
    if let Some(proc) = crate::process::get_process_mut(pid) {
        let pml4 = proc.pml4_phys;
        let mut name_bytes = [0u8; 128];
        let len = core::cmp::min(max_len, 127);
        for i in 0..len {
            let vaddr = vaddr_start + i as u64;
            if let Some(phys) = crate::memory::paging::get_mapped_frame(pml4, vaddr) {
                let byte = unsafe { *(crate::memory::paging::phys_to_virt(phys) as *const u8) };
                name_bytes[i] = byte;
            } else {
                return None; // Page fault while reading
            }
        }
        if let Ok(s) = core::str::from_utf8(&name_bytes[..len]) {
             Some(alloc::string::String::from(s))
        } else {
             None
        }
    } else {
        None
    }
}

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
    let arg4 = ctx.r10;
    let arg5 = ctx.r8;
    let arg6 = ctx.r9;
    let arg7 = ctx.r11; // We can use up to 7 args via registers safely

    match syscall_num {
        SYS_SEND => {
            let to_pid = arg1 as usize;
            let msg_type = arg2;
            crate::serial_println!("[Kernel] SYS_SEND from PID {} to PID {}: type={:#X}", scheduler::current_pid(), to_pid, msg_type);
            let success = ipc::send(to_pid, msg_type, arg3, arg4, arg5, arg6, arg7, 0); // 6 args payload
            ctx.rax = if success { 0 } else { 1 };
            context_ptr
        }
        SYS_RECV => {
            let msg_ptr = arg1 as *mut ipc::Message;
            let non_blocking = arg2 == 1;
            
            if let Some(msg) = ipc::try_receive() {
                unsafe {
                    core::ptr::write(msg_ptr, msg);
                }
                ctx.rax = msg.sender as u64;
                context_ptr
            } else if non_blocking {
                // Non-blocking: return u64::MAX to indicate no message
                ctx.rax = u64::MAX;
                context_ptr
            } else {
                // Blocking: block the process and retry when unblocked
                scheduler::block_current();
                ctx.rip -= 2;
                scheduler::schedule(context_ptr)
            }
        }
        SYS_EXIT => { // 6
            crate::process::exit_process(scheduler::current_pid(), arg1);
            scheduler::schedule(context_ptr)
        }
        SYS_YIELD => {
            scheduler::schedule(context_ptr)
        }
        SYS_GETPID => {
            crate::serial_println!("[Kernel] SYS_GETPID called by PID {}", scheduler::current_pid());
            ctx.rax = scheduler::current_pid() as u64;
            context_ptr
        }
        SYS_MAP_PHYSICAL => {
            let virt = arg1;
            let phys = arg2;
            let size = arg3;
            crate::serial_println!("[Kernel] SYS_MAP_PHYSICAL called by PID {} - virt={:#X} phys={:#X} size={:#X}", scheduler::current_pid(), virt, phys, size);
            
            let start_virt = virt & !0xFFF;
            let start_phys = phys & !0xFFF;
            let end_virt = (virt + size + 0xFFF) & !0xFFF;
            
            let mut v = start_virt;
            let mut p = start_phys;
            
            while v < end_virt {
                crate::memory::paging::map_page(v, p, crate::memory::paging::USER | crate::memory::paging::WRITABLE);
                v += 0x1000;
                p += 0x1000;
            }
            
            ctx.rax = 0; // Success
            context_ptr
        }
        SYS_INB => {
            let port = arg1 as u16;
            let mut val: u8;
            unsafe {
                core::arch::asm!("in al, dx", in("dx") port, out("al") val, options(nomem, nostack));
            }
            ctx.rax = val as u64;
            context_ptr
        }
        SYS_GET_FB_INFO => {
            crate::serial_println!("[Kernel] SYS_GET_FB_INFO called by PID {}", scheduler::current_pid());
            if let Some(response) = crate::FRAMEBUFFER_REQUEST.get_response() {
                if let Some(fb) = response.framebuffers().next() {
                    let fb_ptr = fb.addr() as u64;
                    let paddr = crate::memory::paging::get_mapped_frame(crate::memory::paging::get_pml4(), fb_ptr).unwrap_or(0);
                    ctx.rax = paddr;
                    ctx.rdi = fb.width() as u64;
                    ctx.rsi = fb.height() as u64;
                    ctx.rdx = fb.pitch() as u64;
                    ctx.r10 = (fb.bpp() / 8) as u64;
                    return context_ptr;
                }
            }
            ctx.rax = 0;
            context_ptr
        }
        SYS_SBRK => {
            // arg1 = increment (signed as i64)
            let increment = arg1 as i64;
            let pid = scheduler::current_pid();
            if let Some(proc) = process::get_process_mut(pid) {
                let old_break = proc.heap_break;
                if increment == 0 {
                    ctx.rax = old_break;
                } else if increment > 0 {
                    let new_break = old_break + increment as u64;
                    let new_break_aligned = (new_break + 0xFFF) & !0xFFF;
                    
                    // Register a VMA for the new heap region
                    use crate::vma::{Vma, VmaFlags, VmaBacking};
                    proc.vmas.add(Vma {
                        start: (old_break + 0xFFF) & !0xFFF,
                        end: new_break_aligned,
                        flags: VmaFlags::USER | VmaFlags::READ | VmaFlags::WRITE,
                        backing: VmaBacking::Anonymous,
                    });
                    
                    proc.heap_break = new_break;
                    ctx.rax = old_break; // Return the OLD break (like POSIX sbrk)
                } else {
                    // Shrinking not supported yet
                    ctx.rax = old_break;
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_MMAP => {
            // arg1 = hint address (0 = kernel picks), arg2 = size, arg3 = flags
            let size = arg2;
            let pid = scheduler::current_pid();
            if let Some(proc) = process::get_process_mut(pid) {
                let aligned_size = (size + 0xFFF) & !0xFFF;
                let hint = if arg1 != 0 { arg1 } else { 0x2000_0000 };
                
                if let Some(addr) = proc.vmas.find_free_region(hint, aligned_size) {
                    use crate::vma::{Vma, VmaFlags, VmaBacking};
                    proc.vmas.add(Vma {
                        start: addr,
                        end: addr + aligned_size,
                        flags: VmaFlags::USER | VmaFlags::READ | VmaFlags::WRITE,
                        backing: VmaBacking::Anonymous,
                    });
                    ctx.rax = addr;
                } else {
                    ctx.rax = u64::MAX;
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_MUNMAP => {
            // arg1 = address, arg2 = size
            let addr = arg1 & !0xFFF;
            let size = (arg2 + 0xFFF) & !0xFFF;
            let pid = scheduler::current_pid();
            if let Some(proc) = process::get_process_mut(pid) {
                // Remove matching VMA
                proc.vmas.regions.retain(|v| !(v.start == addr && v.end == addr + size));
                // Unmap pages
                let mut page = addr;
                while page < addr + size {
                    crate::memory::paging::unmap_page(page);
                    page += 0x1000;
                }
                ctx.rax = 0;
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_SHM_CREATE => {
            let size = arg1 as usize;
            let pid = scheduler::current_pid();
            if let Some(id) = crate::shm::create(size, pid) {
                ctx.rax = id as u64;
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_SHM_MAP => {
            let shm_id = arg1 as u32;
            let virt_addr = arg2;
            let pid = scheduler::current_pid();
            if let Some(proc) = process::get_process_mut(pid) {
                match crate::shm::map(shm_id, proc.pml4_phys, virt_addr) {
                    Ok(size) => ctx.rax = size as u64,
                    Err(_) => ctx.rax = u64::MAX,
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_SHM_UNMAP => {
            let shm_id = arg1 as u32;
            let virt_addr = arg2;
            let pid = scheduler::current_pid();
            if let Some(proc) = process::get_process_mut(pid) {
                match crate::shm::unmap(shm_id, proc.pml4_phys, virt_addr) {
                    Ok(_) => ctx.rax = 0,
                    Err(_) => ctx.rax = u64::MAX,
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_SPAWN => {
            // arg1 = filename_ptr (in user address space), arg2 = filename_len
            let filename_ptr = arg1;
            let filename_len = arg2 as usize;
            let pid = scheduler::current_pid();
            
            let filename = read_user_string(filename_ptr, filename_len, pid);
            if let Some(filename) = filename {
                match crate::fat32::read_file(&filename) {
                    Some(data) => {
                        crate::serial_println!("[SPAWN] Loading '{}' ({} bytes)", filename, data.len());
                        if let Some(child_pid) = crate::process::load_elf("elf_app", &data, pid) {
                            crate::serial_println!("[SPAWN] Spawned PID {} for '{}'", child_pid, filename);
                            ctx.rax = child_pid as u64;
                        } else {
                            crate::serial_println!("[SPAWN] Failed to load ELF '{}'", filename);
                            ctx.rax = u64::MAX;
                        }
                    }
                    None => {
                        crate::serial_println!("[SPAWN] File not found: '{}'", filename);
                        ctx.rax = u64::MAX;
                    }
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_WAITPID => {
            let child_pid = arg1 as usize;
            let current = scheduler::current_pid();
            if let Some(child) = crate::process::get_process_mut(child_pid) {
                if child.state == crate::process::ProcessState::Dead {
                    // Child is dead, reap it
                    let code = child.exit_code.unwrap_or(0);
                    child.state = crate::process::ProcessState::Empty; // Free slot
                    ctx.rax = code;
                    context_ptr
                } else {
                    // Child is still alive, block parent
                    if let Some(parent) = crate::process::get_process_mut(current) {
                        parent.state = crate::process::ProcessState::Blocked;
                        parent.waiting_for_pid = Some(child_pid);
                    }
                    // Rewind RIP so syscall executes again when unblocked
                    ctx.rip -= 2;
                    scheduler::schedule(context_ptr)
                }
            } else {
                // Child doesn't exist (already reaped or invalid pid)
                ctx.rax = u64::MAX;
                context_ptr
            }
        }
        SYS_EXECVE => {
            let filename_ptr = arg1;
            let filename_len = arg2 as usize;
            let pid = scheduler::current_pid();
            
            let filename = read_user_string(filename_ptr, filename_len, pid);
            if let Some(filename) = filename {
                match crate::fat32::read_file(&filename) {
                    Some(data) => {
                        crate::serial_println!("[EXECVE] Replacing PID {} with '{}'", pid, filename);
                        // For now, replacing the address space natively is tricky.
                        // We will just print failure and return MAX because microkernels prefer spawn.
                        // Since we implemented SPAWN, EXECVE is less critical but stubbed here natively if needed later.
                        ctx.rax = u64::MAX;
                    }
                    None => ctx.rax = u64::MAX,
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_OPEN => {
            // arg1 = filename_ptr, arg2 = filename_len, arg3 = flags (0=RD, 1=WR)
            let pid = scheduler::current_pid();
            let filename = read_user_string(arg1, arg2 as usize, pid);
            if let Some(filename) = filename {
                if let Some(proc) = process::get_process_mut(pid) {
                    match proc.fd_table.open(&filename, arg3) {
                        Some(fd) => {
                            crate::serial_println!("[VFS] PID {} opened '{}' as fd {}", pid, filename, fd);
                            ctx.rax = fd as u64;
                        }
                        None => {
                            crate::serial_println!("[VFS] PID {} failed to open '{}'", pid, filename);
                            ctx.rax = u64::MAX;
                        }
                    }
                } else {
                    ctx.rax = u64::MAX;
                }
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_READ => {
            // arg1 = fd, arg2 = user_buf_ptr, arg3 = count
            let pid = scheduler::current_pid();
            let fd = arg1 as usize;
            let user_buf = arg2;
            let count = arg3 as usize;

            if let Some(proc) = process::get_process_mut(pid) {
                let mut tmp_buf = alloc::vec![0u8; count.min(4096)];
                let bytes_read = proc.fd_table.read(fd, &mut tmp_buf, count.min(4096));
                if bytes_read > 0 {
                    // Write bytes back to userspace
                    write_user_bytes(proc.pml4_phys, user_buf, &tmp_buf[..bytes_read]);
                }
                ctx.rax = bytes_read as u64;
            } else {
                ctx.rax = 0;
            }
            context_ptr
        }
        SYS_WRITE_FILE => {
            // arg1 = fd, arg2 = user_buf_ptr, arg3 = count
            let pid = scheduler::current_pid();
            let fd = arg1 as usize;
            let user_buf = arg2;
            let count = arg3 as usize;

            if let Some(proc) = process::get_process_mut(pid) {
                let pml4 = proc.pml4_phys;
                // Read bytes from userspace
                let mut tmp_buf = alloc::vec![0u8; count.min(4096)];
                read_user_bytes(pml4, user_buf, &mut tmp_buf[..count.min(4096)]);
                let bytes_written = proc.fd_table.write(fd, &tmp_buf[..count.min(4096)], count.min(4096));
                ctx.rax = bytes_written as u64;
            } else {
                ctx.rax = 0;
            }
            context_ptr
        }
        SYS_CLOSE => {
            // arg1 = fd
            let pid = scheduler::current_pid();
            let fd = arg1 as usize;
            if let Some(proc) = process::get_process_mut(pid) {
                ctx.rax = if proc.fd_table.close(fd) { 0 } else { u64::MAX };
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_STAT => {
            // arg1 = fd. Returns file size.
            let pid = scheduler::current_pid();
            let fd = arg1 as usize;
            if let Some(proc) = process::get_process_mut(pid) {
                ctx.rax = proc.fd_table.stat(fd);
            } else {
                ctx.rax = u64::MAX;
            }
            context_ptr
        }
        SYS_LISTDIR => {
            // arg1 = user_buf_ptr, arg2 = buf_size
            // Writes null-terminated list of filenames separated by newlines
            let pid = scheduler::current_pid();
            let user_buf = arg1;
            let buf_size = arg2 as usize;

            let files = crate::fat32::list_files();
            let mut output = alloc::string::String::new();
            for (name, size) in &files {
                use core::fmt::Write;
                let _ = write!(output, "{} {}\n", name, size);
            }
            let bytes = output.as_bytes();
            let to_copy = bytes.len().min(buf_size);

            if let Some(proc) = process::get_process_mut(pid) {
                write_user_bytes(proc.pml4_phys, user_buf, &bytes[..to_copy]);
            }
            ctx.rax = to_copy as u64;
            context_ptr
        }
        SYS_SHUTDOWN => {
            crate::serial_println!("[Kernel] SYS_SHUTDOWN called by PID {} — Powering off via ACPI!", scheduler::current_pid());
            // QEMU ACPI shutdown: write 0x2000 to port 0x604
            unsafe {
                core::arch::asm!(
                    "out dx, ax",
                    in("dx") 0x604u16,
                    in("ax") 0x2000u16,
                    options(nomem, nostack, preserves_flags)
                );
            }
            // If we're still running, loop forever
            loop { core::hint::spin_loop(); }
        }
        SYS_GET_TIME => {
            let dt = crate::rtc::read_datetime();
            ctx.rax = dt.hour as u64;
            ctx.rdi = dt.minute as u64;
            ctx.rsi = dt.second as u64;
            ctx.rdx = dt.day as u64;
            ctx.r10 = dt.month as u64;
            ctx.r8 = dt.year as u64;
            context_ptr
        }
        _ => {
            ctx.rax = u64::MAX;
            context_ptr
        }
    }
}

/// Helper: write bytes into a userspace address via the process's page table.
fn write_user_bytes(pml4: u64, vaddr_start: u64, data: &[u8]) {
    for (i, &byte) in data.iter().enumerate() {
        let vaddr = vaddr_start + i as u64;
        if let Some(phys) = crate::memory::paging::get_mapped_frame(pml4, vaddr) {
            let offset = (vaddr & 0xFFF) as usize;
            let ptr = crate::memory::paging::phys_to_virt(phys) as *mut u8;
            unsafe { *ptr.add(offset) = byte; }
        }
    }
}

/// Helper: read bytes from a userspace address via the process's page table.
fn read_user_bytes(pml4: u64, vaddr_start: u64, buf: &mut [u8]) {
    for (i, byte) in buf.iter_mut().enumerate() {
        let vaddr = vaddr_start + i as u64;
        if let Some(phys) = crate::memory::paging::get_mapped_frame(pml4, vaddr) {
            let offset = (vaddr & 0xFFF) as usize;
            let ptr = crate::memory::paging::phys_to_virt(phys) as *const u8;
            *byte = unsafe { *ptr.add(offset) };
        }
    }
}
