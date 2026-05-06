#![allow(dead_code)]

/// 64-bit ELF Header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64_Ehdr {
    pub e_ident: [u8; 16], // Magic number and other info
    pub e_type: u16,       // Object file type
    pub e_machine: u16,    // Architecture
    pub e_version: u32,    // Object file version
    pub e_entry: u64,      // Entry point virtual address
    pub e_phoff: u64,      // Program header table file offset
    pub e_shoff: u64,      // Section header table file offset
    pub e_flags: u32,      // Processor-specific flags
    pub e_ehsize: u16,     // ELF header size in bytes
    pub e_phentsize: u16,  // Program header table entry size
    pub e_phnum: u16,      // Program header table entry count
    pub e_shentsize: u16,  // Section header table entry size
    pub e_shnum: u16,      // Section header table entry count
    pub e_shstrndx: u16,   // Section header string table index
}

/// 64-bit ELF Program Header
#[repr(C)]
#[derive(Debug, Clone, Copy)]
pub struct Elf64_Phdr {
    pub p_type: u32,   // Segment type
    pub p_flags: u32,  // Segment flags
    pub p_offset: u64, // Segment file offset
    pub p_vaddr: u64,  // Segment virtual address
    pub p_paddr: u64,  // Segment physical address
    pub p_filesz: u64, // Segment size in file
    pub p_memsz: u64,  // Segment size in memory
    pub p_align: u64,  // Segment alignment
}

// ELF Magic Signature: 0x7F 'E' 'L' 'F'
const ELF_MAGIC: [u8; 4] = [0x7F, b'E', b'L', b'F'];
const PT_LOAD: u32 = 1;

pub struct ElfParser<'a> {
    data: &'a [u8],
    header: &'a Elf64_Ehdr,
}

impl<'a> ElfParser<'a> {
    pub fn new(data: &'a [u8]) -> Option<Self> {
        let ehdr_size = core::mem::size_of::<Elf64_Ehdr>();
        if data.len() < ehdr_size {
            crate::serial_println!("[ELF ERROR] Data too small");
            return None;
        }

        // Safety: We check the length above, and Elf64_Ehdr is a POD (Plain Old Data) type.
        let header = unsafe { &*(data.as_ptr() as *const Elf64_Ehdr) };

        // Check Magic
        if header.e_ident[0..4] != ELF_MAGIC {
            crate::serial_println!("[ELF ERROR] Invalid magic bytes");
            return None;
        }

        // Check 64-bit (EI_CLASS == 2)
        if header.e_ident[4] != 2 {
            crate::serial_println!("[ELF ERROR] Not 64-bit");
            return None;
        }

        // Check little-endian (EI_DATA == 1)
        if header.e_ident[5] != 1 {
            crate::serial_println!("[ELF ERROR] Not little endian");
            return None;
        }

        // Check Executable (e_type == 2 for EXEC, 3 for DYN/PIE)
        if header.e_type != 2 && header.e_type != 3 {
            crate::serial_println!("[ELF ERROR] Not an executable. type={}", header.e_type);
            return None;
        }

        Some(Self { data, header })
    }

    /// Get the Entry Point Address
    pub fn entry_point(&self) -> u64 {
        self.header.e_entry
    }

    /// Return an iterator over PT_LOAD segments
    pub fn load_segments(&self) -> impl Iterator<Item = &'a Elf64_Phdr> + 'a {
        let phoff = self.header.e_phoff as usize;
        let phnum = self.header.e_phnum as usize;
        let phentsize = self.header.e_phentsize as usize;
        let data = self.data; // Has lifetime 'a

        (0..phnum).filter_map(move |i| {
            let offset = phoff + (i * phentsize);
            if offset + phentsize <= data.len() {
                let phdr = unsafe { &*(data.as_ptr().add(offset) as *const Elf64_Phdr) };
                if phdr.p_type == PT_LOAD {
                    Some(phdr)
                } else {
                    None
                }
            } else {
                None
            }
        })
    }

    /// Get the slice of bytes corresponding to a program segment
    pub fn segment_data(&self, phdr: &Elf64_Phdr) -> &'a [u8] {
        let start = phdr.p_offset as usize;
        let end = start + (phdr.p_filesz as usize);

        if end <= self.data.len() {
            &self.data[start..end]
        } else {
            // Unsafe or corrupt offset/size, return empty
            &[]
        }
    }
}
