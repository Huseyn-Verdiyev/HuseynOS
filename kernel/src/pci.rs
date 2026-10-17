use alloc::string::String;
use alloc::vec::Vec;
use core::arch::asm;
use core::fmt::Write;
use spin::Mutex;
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
    pub msi_cap_offset: u8,
}

static PCI_DEVICES: Mutex<Vec<PciDevice>> = Mutex::new(Vec::new());

unsafe fn outl(port: u16, value: u32) {
    asm!(
        "out dx, eax",
        in("dx") port,
        in("eax") value,
        options(nomem, nostack, preserves_flags)
    );
}

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

pub fn pci_config_write_dword(bus: u8, slot: u8, func: u8, offset: u8, val: u32) {
    let address: u32 = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    unsafe {
        outl(0xCF8, address);
        outl(0xCFC, val);
    }
}

pub fn pci_config_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let tmp = pci_config_read_dword(bus, slot, func, offset);
    ((tmp >> ((offset & 2) * 8)) & 0xFFFF) as u16
}

pub fn class_description(class_id: u8, subclass_id: u8) -> &'static str {
    match (class_id, subclass_id) {
        (0x01, 0x01) => "IDE/ATA Storage",
        (0x01, 0x06) => "AHCI SATA 3.0",
        (0x01, 0x08) => "NVMe PCIe SSD",
        (0x02, 0x00) => "Ethernet NIC",
        (0x03, 0x00) => "VGA/Framebuffer",
        (0x06, 0x00) => "Host Bridge",
        (0x06, 0x01) => "ISA Bridge",
        (0x06, 0x80) => "PCI Bridge",
        (0x0C, 0x03) => "USB xHCI/eHCI",
        _ => "PCI Device",
    }
}

/// Walk PCI capability list to find MSI Capability (ID 0x05)
fn find_msi_capability(bus: u8, slot: u8, func: u8) -> u8 {
    let status = pci_config_read_word(bus, slot, func, 0x06);
    if (status & (1 << 4)) == 0 {
        return 0; // Capabilities list bit not set
    }

    let mut cap_ptr = (pci_config_read_dword(bus, slot, func, 0x34) & 0xFF) as u8;
    for _ in 0..48 {
        if cap_ptr == 0 || cap_ptr & 3 != 0 {
            break;
        }
        let cap_header = pci_config_read_dword(bus, slot, func, cap_ptr);
        let cap_id = (cap_header & 0xFF) as u8;
        if cap_id == 0x05 {
            return cap_ptr;
        }
        cap_ptr = ((cap_header >> 8) & 0xFF) as u8;
    }
    0
}

/// Enable PCI Message Signaled Interrupts (MSI) to route device interrupts to a specific APIC vector.
pub fn enable_pci_msi(bus: u8, slot: u8, func: u8, cap_offset: u8, dest_apic_id: u8, vector: u8) -> bool {
    if cap_offset == 0 {
        return false;
    }
    // MSI Message Address: 0xFEE00000 | (dest_apic_id << 12)
    let msg_addr = 0xFEE0_0000u32 | ((dest_apic_id as u32) << 12);
    let msg_data = vector as u32;

    pci_config_write_dword(bus, slot, func, cap_offset + 0x04, msg_addr);
    pci_config_write_dword(bus, slot, func, cap_offset + 0x08, msg_data);

    // Set MSI Enable (bit 16 of Message Control register)
    let ctrl_dword = pci_config_read_dword(bus, slot, func, cap_offset);
    pci_config_write_dword(bus, slot, func, cap_offset, ctrl_dword | (1 << 16));
    true
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
    let msi_cap = find_msi_capability(bus, slot, func);

    println!(
        "  [PCI {:02X}:{:02X}.{}] {:04X}:{:04X} ({}) BAR0={:#X} MSI={}",
        bus,
        slot,
        func,
        vendor_id,
        device_id,
        class_description(class_id, subclass_id),
        bar0,
        if msi_cap != 0 { "YES" } else { "NO" }
    );

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
        msi_cap_offset: msi_cap,
    })
}

pub fn enumerate_pci() {
    println!("===========================================");
    println!("  HuseynOS PCI Bus & Hardware Enumeration");
    println!("===========================================");

    let mut found_list = Vec::new();

    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor_id = pci_config_read_word(bus, slot, 0, 0x00);
            if vendor_id == 0xFFFF {
                continue;
            }

            if let Some(dev) = check_function(bus, slot, 0) {
                found_list.push(dev);
            }

            let header_type = (pci_config_read_word(bus, slot, 0, 0x0E) & 0xFF) as u8;
            if (header_type & 0x80) != 0 {
                for func in 1..8 {
                    if let Some(dev) = check_function(bus, slot, func) {
                        found_list.push(dev);
                    }
                }
            }
        }
    }

    println!("  [OK] PCI scan complete ({} functions online)", found_list.len());
    println!("===========================================\n");

    let mut global = PCI_DEVICES.lock();
    *global = found_list;
}

pub fn find_ahci_bar5() -> Option<u32> {
    let devs = PCI_DEVICES.lock();
    for d in devs.iter() {
        if d.class_id == 0x01 && d.subclass_id == 0x06 && d.bar5 != 0 {
            return Some(d.bar5);
        }
    }
    None
}

pub fn find_xhci_bar0() -> Option<u32> {
    let devs = PCI_DEVICES.lock();
    for d in devs.iter() {
        if d.class_id == 0x0C && d.subclass_id == 0x03 && d.bar0 != 0 {
            return Some(d.bar0);
        }
    }
    None
}

pub fn find_nvme_bar0() -> Option<u32> {
    let devs = PCI_DEVICES.lock();
    for d in devs.iter() {
        if d.class_id == 0x01 && d.subclass_id == 0x08 && d.bar0 != 0 {
            return Some(d.bar0);
        }
    }
    None
}

pub fn format_pci_list() -> String {
    let devs = PCI_DEVICES.lock();
    let mut out = String::new();
    let _ = writeln!(out, "BDF       VEN:DEV    CLASS  MSI  TYPE");
    for d in devs.iter() {
        let _ = writeln!(
            out,
            "{:02X}:{:02X}.{}   {:04X}:{:04X}  {:02X}/{:02X}  {:<4} {}",
            d.bus,
            d.slot,
            d.func,
            d.vendor_id,
            d.device_id,
            d.class_id,
            d.subclass_id,
            if d.msi_cap_offset != 0 { "YES" } else { "NO" },
            class_description(d.class_id, d.subclass_id)
        );
    }
    out
}
