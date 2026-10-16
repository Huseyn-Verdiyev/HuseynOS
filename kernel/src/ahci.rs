//! Bare-metal AHCI (Advanced Host Controller Interface) SATA 3.0 Driver.
//! Probes the PCI ABAR (BAR5) MMIO registers, enumerates all 32 HBA ports,
//! and identifies attached SATA / SATAPI drives for real-hardware storage support.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use spin::Mutex;
use crate::println;
use crate::memory::paging::{map_page, phys_to_virt, WRITABLE};

const SATA_SIG_ATA: u32   = 0x0000_0101; // SATA drive
const SATA_SIG_ATAPI: u32 = 0xEB14_0101; // SATAPI optical drive
const SATA_SIG_SEMB: u32  = 0xC33C_0101; // Enclosure management bridge
const SATA_SIG_PM: u32    = 0x9669_0101; // Port multiplier

const HBA_PORT_IPM_ACTIVE: u32  = 1;
const HBA_PORT_DET_PRESENT: u32 = 3;

#[derive(Clone, Copy, Debug)]
pub struct AhciPortInfo {
    pub port_num: u8,
    pub kind: &'static str,
    pub sig: u32,
}

static AHCI_PORTS: Mutex<Vec<AhciPortInfo>> = Mutex::new(Vec::new());
static AHCI_ABAR: Mutex<u64> = Mutex::new(0);

/// Initialize an AHCI controller from its PCI BAR5 physical base address.
pub fn init(bar5_raw: u32) {
    let abar_phys = (bar5_raw & !0xF) as u64;
    if abar_phys == 0 {
        return;
    }

    *AHCI_ABAR.lock() = abar_phys;
    let abar_virt = phys_to_virt(abar_phys);

    // Ensure the MMIO region (2 pages = 8 KiB) is mapped in the kernel page table
    map_page(abar_virt, abar_phys, WRITABLE);
    map_page(abar_virt + 0x1000, abar_phys + 0x1000, WRITABLE);

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

        let mut ports = AHCI_PORTS.lock();
        ports.clear();

        for port in 0..32u8 {
            if (pi & (1 << port)) != 0 {
                let port_base = abar_virt + 0x100 + (port as u64) * 0x80;
                let ssts = ((port_base + 0x28) as *const u32).read_volatile();
                let sig = ((port_base + 0x24) as *const u32).read_volatile();

                let det = ssts & 0x0F;
                let ipm = (ssts >> 8) & 0x0F;

                if det == HBA_PORT_DET_PRESENT && ipm == HBA_PORT_IPM_ACTIVE {
                    let kind = match sig {
                        SATA_SIG_ATA => "SATA-DISK",
                        SATA_SIG_ATAPI => "SATAPI-OPTICAL",
                        SATA_SIG_SEMB => "SEMB",
                        SATA_SIG_PM => "PORT-MUL",
                        _ => "SATA-DEV",
                    };
                    println!("  [AHCI] Port {} active -> {} (sig={:#010X})", port, kind, sig);
                    ports.push(AhciPortInfo { port_num: port, kind, sig });
                }
            }
        }
    }
}

pub fn format_status() -> String {
    let abar = *AHCI_ABAR.lock();
    let ports = AHCI_PORTS.lock();
    let mut s = String::new();
    if abar == 0 {
        let _ = writeln!(s, "AHCI: not present (using ATA/PIO + RAM overlay)");
    } else {
        let _ = writeln!(s, "AHCI: HBA @ {:#X} ({} active ports)", abar, ports.len());
        for p in ports.iter() {
            let _ = writeln!(s, "  Port {}: {} (sig={:#010X})", p.port_num, p.kind, p.sig);
        }
    }
    s
}
