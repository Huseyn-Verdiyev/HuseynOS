use alloc::vec::Vec;

/// Flags for virtual memory areas.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VmaFlags(u64);

impl VmaFlags {
    pub const READ: VmaFlags = VmaFlags(1 << 0);
    pub const WRITE: VmaFlags = VmaFlags(1 << 1);
    pub const EXEC: VmaFlags = VmaFlags(1 << 2);
    pub const USER: VmaFlags = VmaFlags(1 << 3);
    pub const SHARED: VmaFlags = VmaFlags(1 << 4);

    pub const fn empty() -> Self { VmaFlags(0) }

    pub const fn contains(&self, other: VmaFlags) -> bool {
        self.0 & other.0 == other.0
    }

    pub const fn bits(&self) -> u64 { self.0 }
}

impl core::ops::BitOr for VmaFlags {
    type Output = VmaFlags;
    fn bitor(self, rhs: VmaFlags) -> VmaFlags {
        VmaFlags(self.0 | rhs.0)
    }
}

/// What backs this VMA region.
#[derive(Debug, Clone)]
pub enum VmaBacking {
    /// Zero-filled on demand (stack, heap, BSS).
    Anonymous,
    /// Backed by ELF segment data.
    ElfSegment {
        /// Raw ELF data (we keep a copy of segment bytes).
        data: Vec<u8>,
        /// File offset within the segment for this region.
        file_offset: usize,
        /// How many bytes actually come from the file (rest is zeroed — BSS).
        file_size: usize,
    },
    /// Physical memory mapping (e.g. framebuffer) — not demand-paged.
    PhysicalMap {
        phys_start: u64,
    },
}

/// A Virtual Memory Area — a contiguous region of valid virtual addresses.
#[derive(Debug, Clone)]
pub struct Vma {
    /// Start virtual address (page-aligned).
    pub start: u64,
    /// End virtual address (exclusive, page-aligned).
    pub end: u64,
    /// Access flags.
    pub flags: VmaFlags,
    /// What backs this region.
    pub backing: VmaBacking,
}

impl Vma {
    /// Returns true if address falls within this VMA.
    pub fn contains(&self, addr: u64) -> bool {
        addr >= self.start && addr < self.end
    }

    /// Returns the page-table flags corresponding to this VMA's permissions.
    pub fn page_flags(&self) -> u64 {
        let mut pf = crate::memory::paging::USER; // Always user-accessible
        if self.flags.contains(VmaFlags::WRITE) {
            pf |= crate::memory::paging::WRITABLE;
        }
        pf
    }
}

/// Per-process VMA list.
#[derive(Debug, Clone)]
pub struct VmaList {
    pub regions: Vec<Vma>,
}

impl VmaList {
    pub const fn new() -> Self {
        VmaList { regions: Vec::new() }
    }

    /// Add a new VMA region.
    pub fn add(&mut self, vma: Vma) {
        self.regions.push(vma);
    }

    /// Find the VMA that contains the given virtual address.
    pub fn find(&self, addr: u64) -> Option<&Vma> {
        self.regions.iter().find(|v| v.contains(addr))
    }

    /// Remove all VMAs (used when replacing address space with execve).
    pub fn clear(&mut self) {
        self.regions.clear();
    }

    /// Find a free region in the user address space for mapping `size` bytes.
    /// Searches upward from `hint` address.
    pub fn find_free_region(&self, hint: u64, size: u64) -> Option<u64> {
        let aligned_hint = (hint + 0xFFF) & !0xFFF;
        let aligned_size = (size + 0xFFF) & !0xFFF;
        let mut candidate = aligned_hint;

        // User address space: 0x0010_0000 .. 0x0000_7FFF_0000_0000
        let max_addr: u64 = 0x0000_7FFF_0000_0000;

        loop {
            if candidate + aligned_size > max_addr {
                return None;
            }

            let mut conflict = false;
            for vma in &self.regions {
                // Check if candidate range overlaps with this VMA
                if candidate < vma.end && candidate + aligned_size > vma.start {
                    candidate = (vma.end + 0xFFF) & !0xFFF;
                    conflict = true;
                    break;
                }
            }

            if !conflict {
                return Some(candidate);
            }
        }
    }
}
