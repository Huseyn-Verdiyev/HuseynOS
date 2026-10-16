//! Bare-metal AHCI (Advanced Host Controller Interface) SATA 3.0 DMA Driver.
//! Maps PCI ABAR (BAR5) MMIO registers, enumerates all 32 HBA ports,
//! and executes hardware FIS DMA 48-bit LBA sector reads and writes (`READ_DMA_EXT` / `WRITE_DMA_EXT`).

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use spin::Mutex;
use crate::println;
use crate::memory::frame::FrameAllocator;
use crate::memory::paging::{map_page, phys_to_virt, CACHE_DISABLE, WRITABLE};

const SATA_SIG_ATA: u32   = 0x0000_0101; // SATA hard disk / SSD
const SATA_SIG_ATAPI: u32 = 0xEB14_0101; // SATAPI optical drive
const SATA_SIG_SEMB: u32  = 0xC33C_0101;
const SATA_SIG_PM: u32    = 0x9669_0101;

const HBA_PORT_IPM_ACTIVE: u32  = 1;
const HBA_PORT_DET_PRESENT: u32 = 3;

const HBA_PxCMD_ST: u32  = 0x0001;
const HBA_PxCMD_FRE: u32 = 0x0010;
const HBA_PxCMD_FR: u32  = 0x4000;
const HBA_PxCMD_CR: u32  = 0x8000;

const FIS_TYPE_REG_H2D: u8 = 0x27;
const ATA_CMD_READ_DMA_EXT: u8  = 0x25;
const ATA_CMD_WRITE_DMA_EXT: u8 = 0x35;

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct HbaCmdHeader {
    flags: u16,     // CFL (bits 0..4), Write (bit 6)
    prdtl: u16,     // PRDT entry count
    prdbc: u32,     // Physical region descriptor byte count transferred
    ctba: u64,      // Command table descriptor base physical address
    reserved: [u32; 4],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct HbaPrdtEntry {
    dba: u64,       // Data base physical address
    reserved0: u32,
    dbc_i: u32,     // Byte count (bits 0..21 = count - 1), Interrupt on completion (bit 31)
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct FisRegH2D {
    fis_type: u8,
    pmport_c: u8,
    command: u8,
    featurel: u8,
    lba0: u8,
    lba1: u8,
    lba2: u8,
    device: u8,
    lba3: u8,
    lba4: u8,
    lba5: u8,
    featureh: u8,
    countl: u8,
    counth: u8,
    icc: u8,
    control: u8,
    reserved: [u8; 4],
}

#[derive(Clone, Copy, Debug)]
pub struct AhciPortInfo {
    pub port_num: u8,
    pub kind: &'static str,
    pub sig: u32,
}

struct AhciController {
    abar_phys: u64,
    active_disk_port: Option<u8>,
    dma_Bounce_phys: u64,
    ports: Vec<AhciPortInfo>,
}

static AHCI_CTRL: Mutex<AhciController> = Mutex::new(AhciController {
    abar_phys: 0,
    active_disk_port: None,
    dma_Bounce_phys: 0,
    ports: Vec::new(),
});

/// Initialize an AHCI controller from its PCI BAR5 physical base address.
pub fn init(bar5_raw: u32) {
    let abar_phys = (bar5_raw & !0xF) as u64;
    if abar_phys == 0 {
        return;
    }

    let abar_virt = phys_to_virt(abar_phys);
    map_page(abar_virt, abar_phys, WRITABLE | CACHE_DISABLE);
    map_page(abar_virt + 0x1000, abar_phys + 0x1000, WRITABLE | CACHE_DISABLE);

    let dma_phys = FrameAllocator::alloc().unwrap_or(0);

    unsafe {
        let cap = (abar_virt as *const u32).read_volatile();
        let ghc_ptr = (abar_virt + 0x04) as *mut u32;
        let pi = ((abar_virt + 0x0C) as *const u32).read_volatile();
        let vs = ((abar_virt + 0x10) as *const u32).read_volatile();

        // Enable AHCI mode (GHC.AE = bit 31)
        let ghc = ghc_ptr.read_volatile();
        ghc_ptr.write_volatile(ghc | (1 << 31));

        let num_ports = (cap & 0x1F) + 1;
        let num_cmd_slots = ((cap >> 8) & 0x1F) + 1;
        let major_ver = (vs >> 16) & 0xFFFF;
        let minor_ver = vs & 0xFFFF;

        println!(
            "[AHCI] HBA v{:x}.{:02x} online @ ABAR {:#X} ({} ports, {} cmd slots)",
            major_ver, minor_ver, abar_phys, num_ports, num_cmd_slots
        );

        let mut ctrl = AHCI_CTRL.lock();
        ctrl.abar_phys = abar_phys;
        ctrl.dma_Bounce_phys = dma_phys;
        ctrl.ports.clear();

        for port in 0..32u8 {
            if (pi & (1 << port)) != 0 {
                let port_base = abar_virt + 0x100 + (port as u64) * 0x80;
                let ssts = ((port_base + 0x28) as *const u32).read_volatile();
                let sig = ((port_base + 0x24) as *const u32).read_volatile();

                let det = ssts & 0x0F;
                let ipm = (ssts >> 8) & 0x0F;

                if det == HBA_PORT_DET_PRESENT && ipm == HBA_PORT_IPM_ACTIVE {
                    let kind = match sig {
                        SATA_SIG_ATA => {
                            if ctrl.active_disk_port.is_none() {
                                ctrl.active_disk_port = Some(port);
                            }
                            "SATA-DISK"
                        }
                        SATA_SIG_ATAPI => "SATAPI-OPTICAL",
                        SATA_SIG_SEMB => "SEMB",
                        SATA_SIG_PM => "PORT-MUL",
                        _ => "SATA-DEV",
                    };
                    println!("  [AHCI] Port {} active -> {} (sig={:#010X})", port, kind, sig);
                    ctrl.ports.push(AhciPortInfo { port_num: port, kind, sig });
                }
            }
        }
    }
}

pub fn is_available() -> bool {
    let ctrl = AHCI_CTRL.lock();
    ctrl.active_disk_port.is_some() && ctrl.dma_Bounce_phys != 0
}

/// Execute a 48-bit LBA DMA sector transfer (Read or Write) on the active AHCI SATA port.
unsafe fn issue_dma_sector(lba: u64, write: bool) -> bool {
    let (abar_phys, port, dma_phys) = {
        let ctrl = AHCI_CTRL.lock();
        match ctrl.active_disk_port {
            Some(p) if ctrl.abar_phys != 0 && ctrl.dma_Bounce_phys != 0 => (ctrl.abar_phys, p, ctrl.dma_Bounce_phys),
            _ => return false,
        }
    };

    let abar_virt = phys_to_virt(abar_phys);
    let port_base = abar_virt + 0x100 + (port as u64) * 0x80;

    let clb_lo = (port_base as *const u32).read_volatile() as u64;
    let clb_hi = ((port_base + 0x04) as *const u32).read_volatile() as u64;
    let clb_phys = clb_lo | (clb_hi << 32);
    if clb_phys == 0 {
        return false;
    }

    // Ensure command list and command table are mapped and start command engine
    let cmd_Ptr = (port_base + 0x18) as *mut u32;
    let cmd_val = cmd_Ptr.read_volatile();
    if (cmd_val & (HBA_PxCMD_ST | HBA_PxCMD_FRE)) != (HBA_PxCMD_ST | HBA_PxCMD_FRE) {
        cmd_Ptr.write_volatile(cmd_val | HBA_PxCMD_FRE | HBA_PxCMD_ST);
    }

    // Clear pending interrupt status
    let is_ptr = (port_base + 0x10) as *mut u32;
    is_ptr.write_volatile(0xFFFF_FFFF);

    let clb_virt = phys_to_virt(clb_phys);
    let hdr = &mut *(clb_virt as *mut HbaCmdHeader);

    // Command Table placed at offset 2048 inside our allocated 4 KiB DMA page (128-byte aligned)
    let cmd_tbl_phys = dma_phys + 2048;
    let cmd_tbl_virt = phys_to_virt(cmd_tbl_phys);
    core::ptr::write_bytes(cmd_tbl_virt as *mut u8, 0, 256);

    let cfl = (core::mem::size_of::<FisRegH2D>() / 4) as u16;
    hdr.flags = cfl | if write { 1 << 6 } else { 0 };
    hdr.prdtl = 1;
    hdr.prdbc = 0;
    hdr.ctba = cmd_tbl_phys;

    // Fill PRDT Entry 0 at offset 0x80 of Command Table (pointing to first 512 bytes of dma_phys)
    let prdt = &mut *((cmd_tbl_virt + 0x80) as *mut HbaPrdtEntry);
    prdt.dba = dma_phys;
    prdt.reserved0 = 0;
    prdt.dbc_i = 511; // 512 bytes - 1

    // Fill Host-to-Device FIS at offset 0x00 of Command Table
    let fis = &mut *(cmd_tbl_virt as *mut FisRegH2D);
    fis.fis_type = FIS_TYPE_REG_H2D;
    fis.pmport_c = 1 << 7; // Command bit set
    fis.command = if write { ATA_CMD_WRITE_DMA_EXT } else { ATA_CMD_READ_DMA_EXT };
    fis.device = 1 << 6;   // LBA mode
    fis.lba0 = (lba & 0xFF) as u8;
    fis.lba1 = ((lba >> 8) & 0xFF) as u8;
    fis.lba2 = ((lba >> 16) & 0xFF) as u8;
    fis.lba3 = ((lba >> 24) & 0xFF) as u8;
    fis.lba4 = ((lba >> 32) & 0xFF) as u8;
    fis.lba5 = ((lba >> 40) & 0xFF) as u8;
    fis.countl = 1;
    fis.counth = 0;

    // Wait until port is not busy (TFD BSY=bit 7, DRQ=bit 3)
    let tfd_ptr = (port_base + 0x20) as *const u32;
    for _ in 0..100_000 {
        if (tfd_ptr.read_volatile() & 0x88) == 0 {
            break;
        }
    }

    // Issue Command Slot 0 (CI = offset 0x38)
    let ci_ptr = (port_base + 0x38) as *mut u32;
    ci_ptr.write_volatile(1);

    for _ in 0..500_000 {
        if (ci_ptr.read_volatile() & 1) == 0 {
            return true;
        }
        if (is_ptr.read_volatile() & (1 << 30)) != 0 {
            return false; // Task file error
        }
    }
    false
}

/// Read a 512-byte sector via AHCI DMA.
pub fn read_sector(lba: u64, buf: &mut [u8; 512]) -> bool {
    if !is_available() {
        return false;
    }
    unsafe {
        if !issue_dma_sector(lba, false) {
            return false;
        }
        let dma_phys = AHCI_CTRL.lock().dma_Bounce_phys;
        let src = phys_to_virt(dma_phys) as *const u8;
        core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), 512);
        true
    }
}

/// Write a 512-byte sector via AHCI DMA.
pub fn write_sector(lba: u64, buf: &[u8; 512]) -> bool {
    if !is_available() {
        return false;
    }
    unsafe {
        let dma_phys = AHCI_CTRL.lock().dma_Bounce_phys;
        let dst = phys_to_virt(dma_phys) as *mut u8;
        core::ptr::copy_nonoverlapping(buf.as_ptr(), dst, 512);
        let _ = HBA_PxCMD_CR;
        let _ = HBA_PxCMD_FR;
        issue_dma_sector(lba, true)
    }
}

pub fn format_status() -> String {
    let ctrl = AHCI_CTRL.lock();
    let mut s = String::new();
    if ctrl.abar_phys == 0 {
        let _ = writeln!(s, "AHCI SATA    : Not present (using ATA/PIO + RAM overlay)");
    } else {
        let _ = writeln!(
            s,
            "AHCI SATA    : HBA @ {:#X} ({} active ports, DMADisk={:?})",
            ctrl.abar_phys,
            ctrl.ports.len(),
            ctrl.active_disk_port
        );
        for p in ctrl.ports.iter() {
            let _ = writeln!(s, "  Port #{}: {} (sig={:#010X})", p.port_num, p.kind, p.sig);
        }
    }
    s
}
