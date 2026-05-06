use core::arch::asm;

/// GDT Entry — 8-byte segment descriptor for x86_64.
#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct GdtEntry {
    limit_low: u16,
    base_low: u16,
    base_mid: u8,
    access: u8,
    flags_limit_high: u8,
    base_high: u8,
}

impl GdtEntry {
    const fn null() -> Self {
        Self {
            limit_low: 0,
            base_low: 0,
            base_mid: 0,
            access: 0,
            flags_limit_high: 0,
            base_high: 0,
        }
    }

    /// Create a code/data segment descriptor.
    /// In long mode, most fields are ignored — only access byte and L/D/G flags matter.
    const fn new(access: u8, flags: u8) -> Self {
        Self {
            limit_low: 0xFFFF,
            base_low: 0,
            base_mid: 0,
            access,
            flags_limit_high: (flags << 4) | 0x0F,
            base_high: 0,
        }
    }
}

/// TSS Entry — 16 bytes in long mode (spans two GDT slots).
#[repr(C, packed)]
#[derive(Clone, Copy)]
struct TssEntry {
    length: u16,
    base_low: u16,
    base_mid: u8,
    flags1: u8,
    flags2: u8,
    base_high: u8,
    base_upper: u32,
    _reserved: u32,
}

/// Task State Segment for x86_64.
#[repr(C, packed)]
pub struct Tss {
    _reserved0: u32,
    /// Privilege stack table (RSP for ring 0, 1, 2).
    pub rsp: [u64; 3],
    _reserved1: u64,
    /// Interrupt stack table (IST1-IST7).
    pub ist: [u64; 7],
    _reserved2: u64,
    _reserved3: u16,
    /// I/O Map Base Address.
    pub iopb_offset: u16,
}

impl Tss {
    const fn new() -> Self {
        Self {
            _reserved0: 0,
            rsp: [0; 3],
            _reserved1: 0,
            ist: [0; 7],
            _reserved2: 0,
            _reserved3: 0,
            iopb_offset: core::mem::size_of::<Tss>() as u16,
        }
    }
}

/// GDTR — pointer loaded by `lgdt`.
#[repr(C, packed)]
struct GdtPointer {
    limit: u16,
    base: u64,
}

// Segment selectors (byte offsets into GDT)
pub const KERNEL_CODE_SEL: u16 = 0x08; // GDT[1]
pub const KERNEL_DATA_SEL: u16 = 0x10; // GDT[2]
pub const USER_DATA_SEL: u16 = 0x18;   // GDT[3]
pub const USER_CODE_SEL: u16 = 0x20;   // GDT[4]
pub const TSS_SEL: u16 = 0x28;         // GDT[5] (16 bytes, spans GDT[5]+GDT[6])

// GDT: null + kernel code + kernel data + user data + user code + TSS(2 slots) = 7 entries
static mut GDT: [u64; 7] = [0; 7];
static mut TSS: Tss = Tss::new();

// Double fault IST stack (16 KiB)
static mut DOUBLE_FAULT_STACK: [u8; 16384] = [0; 16384];

/// Initialize the Global Descriptor Table.
pub fn init() {
    unsafe {
        // Set up IST1 for double fault handler
        let stack_end = DOUBLE_FAULT_STACK.as_ptr().add(DOUBLE_FAULT_STACK.len()) as u64;
        TSS.ist[0] = stack_end; // IST1
        TSS.rsp[0] = stack_end; // RSP0 — kernel stack for ring 0

        // Build GDT entries
        let null = GdtEntry::null();
        // Kernel Code: Present, DPL=0, Code segment, Executable, Readable
        let kernel_code = GdtEntry::new(0x9A, 0xA); // L=1, D=0 (64-bit), G=1
        // Kernel Data: Present, DPL=0, Data segment, Writable
        let kernel_data = GdtEntry::new(0x92, 0xC); // D/B=1, G=1
        // User Data: Present, DPL=3, Data segment, Writable
        let user_data = GdtEntry::new(0xF2, 0xC);
        // User Code: Present, DPL=3, Code segment, Executable, Readable
        let user_code = GdtEntry::new(0xFA, 0xA);

        // Copy entries as u64
        GDT[0] = core::mem::transmute(null);
        GDT[1] = core::mem::transmute(kernel_code);
        GDT[2] = core::mem::transmute(kernel_data);
        GDT[3] = core::mem::transmute(user_data);
        GDT[4] = core::mem::transmute(user_code);

        // TSS descriptor (16 bytes = 2 GDT slots)
        let tss_ptr = &TSS as *const Tss as u64;
        let tss_size = (core::mem::size_of::<Tss>() - 1) as u64;

        let tss_low: u64 = (tss_size & 0xFFFF)
            | ((tss_ptr & 0xFFFF) << 16)
            | (((tss_ptr >> 16) & 0xFF) << 32)
            | (0x89u64 << 40) // Present, type = Available 64-bit TSS
            | (((tss_size >> 16) & 0xF) << 48)
            | (((tss_ptr >> 24) & 0xFF) << 56);
        let tss_high: u64 = tss_ptr >> 32;

        GDT[5] = tss_low;
        GDT[6] = tss_high;

        // Load GDT
        let gdtr = GdtPointer {
            limit: (core::mem::size_of_val(&GDT) - 1) as u16,
            base: GDT.as_ptr() as u64,
        };
        asm!(
            "lgdt [{}]",
            in(reg) &gdtr,
            options(readonly, nostack, preserves_flags)
        );

        // Reload code segment (far jump)
        asm!(
            "push {sel}",
            "lea {tmp}, [rip + 2f]",
            "push {tmp}",
            "retfq",
            "2:",
            sel = in(reg) KERNEL_CODE_SEL as u64,
            tmp = lateout(reg) _,
            options(preserves_flags),
        );

        // Reload data segment registers
        asm!(
            "mov ds, {sel:x}",
            "mov es, {sel:x}",
            "mov fs, {sel:x}",
            "mov gs, {sel:x}",
            "mov ss, {sel:x}",
            sel = in(reg) KERNEL_DATA_SEL as u16,
            options(nostack, preserves_flags),
        );

        // Load TSS
        asm!(
            "ltr ax",
            in("ax") TSS_SEL,
            options(nostack, preserves_flags)
        );
    }
}

/// Update the TSS ring 0 stack pointer.
/// Must be called on every context switch to ring 3.
pub fn set_tss_stack(stack_ptr: u64) {
    unsafe {
        TSS.rsp[0] = stack_ptr;
    }
}
