use alloc::boxed::Box;
use crate::vma::VmaList;
use crate::vfs::FdTable;

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
    pub ds: u64, pub es: u64,
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
    pub pml4_phys: u64, // The physical address of this process's PML4 table
    pub kernel_stack_top: u64, // The top of the kernel stack, for TSS RSP0
    pub vmas: VmaList,  // Virtual memory areas for demand paging
    pub heap_break: u64, // Current program break for sbrk()
    pub parent_pid: Pid, // PID of the parent process (0 if init/kernel)
    pub exit_code: Option<u64>, // Exit code when the process terminates
    pub waiting_for_pid: Option<Pid>, // PID this process is blocked waiting for
    pub fd_table: FdTable, // Per-process file descriptor table
}

impl Process {
    pub const fn empty() -> Self {
        Self {
            pid: 0,
            state: ProcessState::Empty,
            context: Context::empty(),
            stack: None,
            name: "",
            pml4_phys: 0, // 0 means use the kernel's default PML4
            kernel_stack_top: 0,
            vmas: VmaList::new(),
            heap_break: 0,
            parent_pid: 0,
            exit_code: None,
            waiting_for_pid: None,
            fd_table: FdTable::new(),
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
    crate::process::exit_process(crate::scheduler::current_pid(), 0);
    loop { crate::scheduler::yield_now(); }
}

/// Spawn a new kernel task. `entry` is the function to run.
pub fn spawn(name: &'static str, entry: fn()) -> Pid {
    unsafe {
        let pid = NEXT_PID;
        NEXT_PID += 1;

        // Allocate kernel stack safely on the heap without blowing up the local stack
        let stack_ptr = alloc::alloc::alloc_zeroed(core::alloc::Layout::new::<[u8; KERNEL_STACK_SIZE]>())
            as *mut [u8; KERNEL_STACK_SIZE];
        if stack_ptr.is_null() {
            panic!("Out of memory for kernel stack");
        }
        let stack = Box::from_raw(stack_ptr);
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
        ctx_obj.ds = 0x10; // Kernel Data Segment
        ctx_obj.es = 0x10; // Kernel Data Segment
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
            pml4_phys: 0,
            kernel_stack_top: stack_top,
            vmas: VmaList::new(),
            heap_break: 0,
            parent_pid: 0, // Kernel tasks have no user parent
            exit_code: None,
            waiting_for_pid: None,
            fd_table: FdTable::new(),
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

/// Load an ELF binary as a User Mode (Ring 3) Process with demand paging.
/// We register VMAs for each segment and only eagerly map pages that contain
/// the entry point — the rest is demand-paged via page faults.
pub fn load_elf(name: &'static str, elf_data: &[u8], parent_pid: Pid) -> Option<Pid> {
    use alloc::vec::Vec;
    use crate::vma::{Vma, VmaList, VmaFlags, VmaBacking};

    let parser = crate::elf::ElfParser::new(elf_data)?;
    unsafe {
        let pid = NEXT_PID;
        NEXT_PID += 1;

        // 1. Create a new Page Table (PML4) for this process
        let pml4_phys = crate::memory::paging::create_user_page_table();

        // 2. Register VMAs for ELF segments and eagerly map all pages
        //    (Demand paging for ELF is tricky because we don't keep the ELF data around.
        //     We eagerly map now, but use VMAs for stack/heap which ARE demand-paged.)
        let mut max_vaddr: u64 = 0;
        let mut vmas = VmaList::new();

        for phdr in parser.load_segments() {
            let offset = phdr.p_offset as usize;
            let filesz = phdr.p_filesz as usize;
            let memsz = phdr.p_memsz as usize;
            let vaddr = phdr.p_vaddr;

            let start_page = vaddr & !0xFFF;
            let end_page = (vaddr + memsz as u64 + 0xFFF) & !0xFFF;

            // Track the max virtual address for heap placement
            if end_page > max_vaddr {
                max_vaddr = end_page;
            }

            // Determine VMA flags from ELF segment flags
            let mut flags = VmaFlags::USER | VmaFlags::READ;
            if phdr.p_flags & 0x2 != 0 { // PF_W
                flags = flags | VmaFlags::WRITE;
            }
            if phdr.p_flags & 0x1 != 0 { // PF_X
                flags = flags | VmaFlags::EXEC;
            }

            // Register VMA
            vmas.add(Vma {
                start: start_page,
                end: end_page,
                flags,
                backing: VmaBacking::Anonymous, // we eagerly map below
            });

            // Eagerly map all ELF segment pages (data must be copied now)
            let mut current_vaddr = start_page;
            while current_vaddr < end_page {
                let frame = if let Some(existing_frame) = crate::memory::paging::get_mapped_frame(pml4_phys, current_vaddr) {
                    existing_frame
                } else if let Some(new_frame) = crate::memory::frame::FrameAllocator::alloc() {
                    crate::memory::paging::map_page_in_table(
                        pml4_phys,
                        current_vaddr,
                        new_frame,
                        crate::memory::paging::WRITABLE | crate::memory::paging::USER,
                    );
                    
                    let ptr = crate::memory::paging::phys_to_virt(new_frame) as *mut u8;
                    core::ptr::write_bytes(ptr, 0, crate::memory::frame::PAGE_SIZE);
                    
                    new_frame
                } else {
                    return None;
                };

                let ptr = crate::memory::paging::phys_to_virt(frame) as *mut u8;

                    let start = core::cmp::max(vaddr, current_vaddr);
                    let end_data = core::cmp::min(vaddr + filesz as u64, current_vaddr + crate::memory::frame::PAGE_SIZE as u64);

                    if start < end_data {
                        let copy_len = (end_data - start) as usize;
                        let src_offset = offset + (start - vaddr) as usize;
                        let dst_offset = (start - current_vaddr) as usize;
                        
                        core::ptr::copy_nonoverlapping(
                            elf_data[src_offset..].as_ptr(),
                            ptr.add(dst_offset),
                            copy_len,
                        );
                    }
                current_vaddr += crate::memory::frame::PAGE_SIZE as u64;
            }
        }

        // 3. User Stack — DEMAND PAGED (only register VMA, allocate on fault)
        let user_stack_top: u64 = 0x0000_7FFF_FFFF_0000;
        let stack_size: u64 = 256 * 1024; // 256 KiB virtual stack
        let user_stack_bottom = user_stack_top - stack_size;

        vmas.add(Vma {
            start: user_stack_bottom,
            end: user_stack_top,
            flags: VmaFlags::USER | VmaFlags::READ | VmaFlags::WRITE,
            backing: VmaBacking::Anonymous,
        });

        // Eagerly map only the top 2 pages of the stack (the part touching RSP)
        for i in 0..2u64 {
            let page_vaddr = user_stack_top - (i + 1) * crate::memory::frame::PAGE_SIZE as u64;
            if let Some(frame) = crate::memory::frame::FrameAllocator::alloc() {
                crate::memory::paging::map_page_in_table(
                    pml4_phys,
                    page_vaddr,
                    frame,
                    crate::memory::paging::WRITABLE | crate::memory::paging::USER,
                );
                core::ptr::write_bytes(crate::memory::paging::phys_to_virt(frame) as *mut u8, 0, crate::memory::frame::PAGE_SIZE);
            }
        }

        // 4. Set heap break just after the last ELF segment (page-aligned)
        let heap_break = (max_vaddr + 0xFFF) & !0xFFF;

        // 5. Kernel Stack (for processing syscalls/interrupts for this process)
        let stack_ptr = alloc::alloc::alloc_zeroed(core::alloc::Layout::new::<[u8; KERNEL_STACK_SIZE]>())
            as *mut [u8; KERNEL_STACK_SIZE];
        if stack_ptr.is_null() {
            crate::serial_println!("[ERROR] alloc_zeroed failed for kernel stack");
            return None; // Out of memory
        }
        let stack = Box::from_raw(stack_ptr);
        let stack_base = stack.as_ptr() as u64;
        let mut stack_top = stack_base + KERNEL_STACK_SIZE as u64;
        stack_top &= !0xF;

        // 6. Build InterruptContext
        let context_size = core::mem::size_of::<InterruptContext>() as u64;
        let context_ptr = (stack_top - context_size) as *mut InterruptContext;
        core::ptr::write_bytes(context_ptr, 0, 1);
        
        crate::serial_println!("[DEBUG] Setting up Context...");
        
        let ctx_obj = &mut *context_ptr;
        
        use crate::gdt::{USER_DATA_SEL, USER_CODE_SEL};
        
        ctx_obj.ds = (USER_DATA_SEL | 3) as u64;
        ctx_obj.es = (USER_DATA_SEL | 3) as u64;
        ctx_obj.ss = (USER_DATA_SEL | 3) as u64;
        ctx_obj.rsp = user_stack_top;
        ctx_obj.rflags = 0x202;
        ctx_obj.cs = (USER_CODE_SEL | 3) as u64;
        ctx_obj.rip = parser.entry_point();
        
        let ctx = Context {
            rsp: context_ptr as u64,
        };

        crate::serial_println!("[DEBUG] Building Process struct...");
        
        let proc = Process {
            pid,
            state: ProcessState::Ready,
            context: ctx,
            stack: Some(stack),
            name,
            pml4_phys,
            kernel_stack_top: stack_top,
            vmas,
            heap_break,
            parent_pid,
            exit_code: None,
            waiting_for_pid: None,
            fd_table: FdTable::new(),
        };

        if let Some(frame) = crate::memory::paging::get_mapped_frame(pml4_phys, 0x400000) {
            let ptr = crate::memory::paging::phys_to_virt(frame) as *const u8;
            unsafe {
                crate::serial_println!("[DEBUG] Bytes at 0x400060: {:02x} {:02x} {:02x} {:02x}", 
                    *ptr.add(0x60), *ptr.add(0x61), *ptr.add(0x62), *ptr.add(0x63));
            }
        }

        unsafe {
            for slot in PROCESS_TABLE.iter_mut() {
                if slot.is_none() {
                    *slot = Some(proc);
                    crate::serial_println!("[DEBUG] Successfully spawned process {}", name);
                    return Some(pid);
                }
            }
        }
        crate::serial_println!("[ERROR] PROCESS_TABLE is full");
    }
    None
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

/// Mark a process as Dead, save its exit code, and wake any waiting parent.
pub fn exit_process(pid: Pid, code: u64) {
    unsafe {
        if let Some(p) = get_process_mut(pid) {
            p.state = ProcessState::Dead;
            p.exit_code = Some(code);
            crate::serial_println!("[Process] PID {} exited with code {}", pid, code);
        }

        // Find any process blocked waiting for THIS pid to exit
        for slot in PROCESS_TABLE.iter_mut() {
            if let Some(ref mut p) = slot {
                if p.state == ProcessState::Blocked && p.waiting_for_pid == Some(pid) {
                    p.state = ProcessState::Ready;
                    p.waiting_for_pid = None;
                    crate::serial_println!("[Process] Waking parent PID {}", p.pid);
                }
            }
        }
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
