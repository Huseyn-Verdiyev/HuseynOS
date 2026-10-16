use core::arch::asm;
use crate::println;

#[derive(Clone, Copy, Debug)]
pub struct PciDevice {
    pub bus: u8,
    pub slot: u8,
    pub func: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub class_id: u8,
    pub subclass_id: u8,
    pub prog_if: u8,
    pub bar0: u32,
    pub bar5: u32,
}

/// Writes a 32-bit value to a PCI configuration port
unsafe fn outl(port: u16, value: u32) {
    asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") value,
        options(nomem, nostack, preserves_flags)
    );
}

/// Reads a 32-bit value from a PCI configuration port
unsafe fn inl(port: u16) -> u32 {
    let value: u32;
    asm!(
        "in eax, dx",
        out("eax") value,
        in("dx") port,
        options(nomem, nostack, preserves_flags)
    );
    value
}

/// Reads a 32-bit dword from PCI configuration space
pub fn pci_config_read_dword(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    unsafe {
        outl(0xCF8, address);
        inl(0xCFC)
    }
}

/// Reads a 16-bit word from PCI configuration space
pub fn pci_config_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let tmp = pci_config_read_dword(bus, slot, func, offset);
    ((tmp >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

fn check_function(bus: u8, slot: u8, func: u8) -> Option<PciDevice> {
    let vendor_id = pci_config_read_word(bus, slot, func, 0x00);
    if vendor_id == 0xFFFF {
        return None;
    }

    let device_id = pci_config_read_word(bus, slot, func, 0x02);
    let class_word = pci_config_read_word(bus, slot, func, 0x0A);
    let rev_progif = pci_config_read_word(bus, slot, func, 0x08);
    let class_id = (class_word >> 8) as u8;
    let subclass_id = (class_word & 0xFF) as u8;
    let prog_if = (rev_progif >> 8) as u8;

    let bar0 = pci_config_read_dword(bus, slot, func, 0x10);
    let bar5 = pci_config_read_dword(bus, slot, func, 0x24);

    println!(
        "  [PCI {:02X}:{:02X}.{}] Vendor={:#06X} Device={:#06X} Class={:#04X}/{:#04X} BAR0={:#010X}",
        bus, slot, func, vendor_id, device_id, class_id, subclass_id, bar0
    );

    match (class_id, subclass_id) {
        (0x01, 0x01) => println!("    └─ [STORAGE] IDE/ATA Controller detected"),
        (0x01, 0x06) => println!("    └─ [STORAGE] AHCI SATA 3.0 Controller detected (ABAR={:#010X})", bar5),
        (0x01, 0x08) => println!("    └─ [STORAGE] NVMe Non-Volatile Memory Controller detected"),
        (0x02, 0x00) => println!("    └─ [NETWORK] Ethernet Controller detected"),
        (0x03, 0x00) => println!("    └─ [DISPLAY] VGA/Framebuffer Compatible Controller"),
        (0x0C, 0x03) => println!("    └─ [USB]     xHCI/eHCI Universal Serial Bus Controller (ProgIF={:#04X})", prog_if),
        _ => {}
    }

    Some(PciDevice {
        bus,
        slot,
        func,
        vendor_id,
        device_id,
        class_id,
        subclass_id,
        prog_if,
        bar0,
        bar5,
    })
}

pub fn enumerate_pci() {
    println!("===========================================");
    println!("  HuseynOS PCI Bus & Hardware Enumeration");
    println!("===========================================");

    let mut found_devices = 0;

    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor_id = pci_config_read_word(bus, slot, 0, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }

            if check_function(bus, slot, 0).is_some() {
                found_devices += 1;
            }

            // Check Header Type bit 7 (multi-function device)
            let header_type = (pci_config_read_word(bus, slot, 0, 0x0E) & 0xFF) as u8;
            if (header_type & 0x80) != 0 {
                for func in 1..8 {
                    if check_function(bus, slot, func).is_some() {
                        found_devices += 1;
                    }
                }
            }
        }
    }

    println!("  [OK] PCI scan complete ({} hardware functions online)", found_devices);
    println!("===========================================\n");
}
