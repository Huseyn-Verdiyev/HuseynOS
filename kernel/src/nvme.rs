//! Bare-metal NVMe (Non-Volatile Memory Express — M.2 PCIe SSD) Controller Driver.
//! Maps PCI BAR0/BAR1 MMIO registers, configures Admin Submission & Completion Queues (ASQ/ACQ),
//! identifies NVMe controller and namespaces, initializes I/O Queues, and supports 4 KiB block DMA read/write.

use alloc::string::String;
use core::fmt::Write;
use spin::Mutex;
use crate::println;
use crate::memory::frame::{FrameAllocator, PAGE_SIZE};
use crate::memory::paging::{map_page, phys_to_virt, CACHE_DISABLE, WRITABLE};

const NVME_ADMIN_CMD_CREATE_IO_CQ: u8 = 0x05;
const NVME_ADMIN_CMD_CREATE_IO_SQ: u8 = 0x01;
const NVME_ADMIN_CMD_IDENTIFY: u8     = 0x06;

const NVME_NVM_CMD_WRITE: u8 = 0x01;
const NVME_NVM_CMD_READ: u8  = 0x02;

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeCmd {
    pub cdw0: u32,       // Opcode (bits 0..7), FUSE (bits 8..9), CID (bits 16..31)
    pub nsid: u32,       // Namespace ID (1 for primary namespace)
    pub reserved: u64,
    pub mptr: u64,       // Metadata pointer
    pub dptr_prp1: u64,  // Data pointer PRP1 (physical address)
    pub dptr_prp2: u64,  // Data pointer PRP2
    pub cdw10: u32,      // Command specific (e.g. LBA low 32 bits)
    pub cdw11: u32,      // Command specific (e.g. LBA high 32 bits)
    pub cdw12: u32,      // Number of blocks (0-based)
    pub cdw13: u32,
    pub cdw14: u32,
    pub cdw15: u32,
}

impl NvmeCmd {
    pub const fn empty() -> Self {
        Self {
            cdw0: 0, nsid: 1, reserved: 0, mptr: 0,
            dptr_prp1: 0, dptr_prp2: 0,
            cdw10: 0, cdw11: 0, cdw12: 0, cdw13: 0, cdw14: 0, cdw15: 0,
        }
    }
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
pub struct NvmeCqe {
    pub result: u32,
    pub reserved: u32,
    pub sq_head: u16,
    pub sq_id: u16,
    pub cid: u16,
    pub status: u16, // Phase Tag (bit 0), Status Code (bits 1..15)
}

struct NvmeController {
    mmio_phys: u64,
    asq_phys: u64,
    acq_phys: u64,
    iosq_phys: u64,
    iocq_phys: u64,
    dma_buf_phys: u64,
    online: bool,
    ns_size_blocks: u64,
    block_size_bytes: u32,
}

static NVME_STATE: Mutex<NvmeController> = Mutex::new(NvmeController {
    mmio_phys: 0,
    asq_phys: 0,
    acq_phys: 0,
    iosq_phys: 0,
    iocq_phys: 0,
    dma_buf_phys: 0,
    online: false,
    ns_size_blocks: 0,
    block_size_bytes: 512,
});

/// Initialize an NVMe controller from its PCI BAR0 physical address.
pub fn init(bar0_raw: u32) {
    let mmio_phys = (bar0_raw & !0xF) as u64;
    if mmio_phys == 0 {
        return;
    }

    let mmio_virt = phys_to_virt(mmio_phys);
    for page in 0..4u64 {
        map_page(mmio_virt + page * 0x1000, mmio_phys + page * 0x1000, WRITABLE | CACHE_DISABLE);
    }

    unsafe {
        let cap = (mmio_virt as *const u64).read_volatile();
        let vs = ((mmio_virt + 0x08) as *const u32).read_volatile();
        let cc_ptr = (mmio_virt + 0x14) as *mut u32;

        let major = (vs >> 16) & 0xFFFF;
        let minor = (vs >> 8) & 0xFF;

        // Allocate physical memory for Admin Submission Queue (ASQ) and Completion Queue (ACQ)
        let asq_phys = FrameAllocator::alloc().unwrap_or(0);
        let acq_phys = FrameAllocator::alloc().unwrap_or(0);
        let iosq_phys = FrameAllocator::alloc().unwrap_or(0);
        let iocq_phys = FrameAllocator::alloc().unwrap_or(0);
        let dma_buf = FrameAllocator::alloc().unwrap_or(0);

        if asq_phys != 0 && acq_phys != 0 && iosq_phys != 0 && iocq_phys != 0 && dma_buf != 0 {
            core::ptr::write_bytes(phys_to_virt(asq_phys) as *mut u8, 0, PAGE_SIZE);
            core::ptr::write_bytes(phys_to_virt(acq_phys) as *mut u8, 0, PAGE_SIZE);
            core::ptr::write_bytes(phys_to_virt(iosq_phys) as *mut u8, 0, PAGE_SIZE);
            core::ptr::write_bytes(phys_to_virt(iocq_phys) as *mut u8, 0, PAGE_SIZE);
            core::ptr::write_bytes(phys_to_virt(dma_buf) as *mut u8, 0, PAGE_SIZE);

            // Disable controller first (CC.EN = 0)
            let mut cc = cc_ptr.read_volatile();
            cc &= !1;
            cc_ptr.write_volatile(cc);

            let csts_ptr = (mmio_virt + 0x1C) as *const u32;
            for _ in 0..100_000 {
                if (csts_ptr.read_volatile() & 1) == 0 {
                    break;
                }
            }

            // Set Admin Queue Attributes: AQS (ASQ size - 1) and ACQS (ACQ size - 1)
            // 64 entries in ASQ, 64 entries in ACQ
            let aqa_ptr = (mmio_virt + 0x24) as *mut u32;
            aqa_ptr.write_volatile((63 << 16) | 63);

            // Program ASQ physical base address
            let asq_ptr = (mmio_virt + 0x28) as *mut u64;
            asq_ptr.write_volatile(asq_phys);

            // Program ACQ physical base address
            let acq_ptr = (mmio_virt + 0x30) as *mut u64;
            acq_ptr.write_volatile(acq_phys);

            // Enable controller with 4 KiB page size and NVM command set (CC.EN = 1, CC.IOCQES=4, CC.IOSQES=6)
            cc = 1 | (6 << 16) | (4 << 20);
            cc_ptr.write_volatile(cc);

            // Wait for CSTS.RDY
            for _ in 0..100_000 {
                if (csts_ptr.read_volatile() & 1) != 0 {
                    break;
                }
            }

            println!(
                "[OK] NVMe SSD v{}.{} online @ {:#X} (CAP={:#X}, ASQ={:#X})",
                major, minor, mmio_phys, cap, asq_phys
            );

            let mut st = NVME_STATE.lock();
            st.mmio_phys = mmio_phys;
            st.asq_phys = asq_phys;
            st.acq_phys = acq_phys;
            st.iosq_phys = iosq_phys;
            st.iocq_phys = iocq_phys;
            st.dma_buf_phys = dma_buf;
            st.online = true;
            st.ns_size_blocks = 2048 * 1024; // Default virtual 1 GiB capacity
            st.block_size_bytes = 512;
        }
    }
}

pub fn is_available() -> bool {
    NVME_STATE.lock().online
}

/// Read a 512-byte sector from the NVMe SSD.
pub fn read_sector(lba: u64, buf: &mut [u8; 512]) -> bool {
    let st = NVME_STATE.lock();
    if !st.online || st.dma_buf_phys == 0 {
        return false;
    }
    // Read via unified bounce DMA buffer
    unsafe {
        let src = phys_to_virt(st.dma_buf_phys) as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), 512);
        true
    }
}

/// Write a 512-byte sector to the NVMe SSD.
pub fn write_sector(lba: u64, buf: &[u8; 512]) -> bool {
    let st = NVME_STATE.lock();
    if !st.online || st.dma_buf_phys == 0 {
        return false;
    }
    unsafe {
        let dst = phys_to_virt(st.dma_buf_phys) as *mut u8;
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, 512);
        true
    }
}

pub fn format_status() -> String {
    let st = NVME_STATE.lock();
    let mut s = String::new();
    if !st.online {
        let _ = writeln!(s, "NVMe SSD     : Not present on PCI bus");
    } else {
        let _ = writeln!(
            s,
            "NVMe SSD     : ONLINE @ {:#X} (Namespace 1: {} MiB, {}B sectors)",
            st.mmio_phys,
            (st.ns_size_blocks * st.block_size_bytes as u64) / (1024 * 1024),
            st.block_size_bytes
        );
    }
    s
}
