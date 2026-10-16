use core::arch::asm;
use crate::println;

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

/// Reads a 16-bit word from PCI configuration space
fn pci_config_read_word(bus: u8, slot: u8, func: u8, offset: u8) -> u16 {
    let address: u32 = (1u32 << 31)
        | ((bus as u32) << 16)
        | ((slot as u32) << 11)
        | ((func as u32) << 8)
        | (offset as u32 & 0xFC);

    unsafe {
        outl(0xCF8, address); // CONFIG_ADDRESS port
        let tmp = inl(0xCFC); // CONFIG_DATA port
        ((tmp >> ((offset & 2) * 8)) & 0xFFFF) as u16
    }
}

pub fn enumerate_pci() {
    println!("===========================================");
    println!("🔍 HuseynOS PCI Bus Initialization");
    println!("===========================================");

    let mut found_devices = 0;

    for bus in 0..=255 {
        for slot in 0..32 {
            let vendor_id = pci_config_read_word(bus, slot, 0, 0);
            
            if vendor_id != 0xFFFF {
                found_devices += 1;
                let device_id = pci_config_read_word(bus, slot, 0, 2);
                let class_subclass = pci_config_read_word(bus, slot, 0, 10);
                
                let class_id = (class_subclass >> 8) as u8;
                let subclass_id = (class_subclass & 0xFF) as u8;

                println!("  ✅ [Bus {}, Slot {}] Detected -> Vendor: 0x{:04X}, Device: 0x{:04X}", 
                    bus, slot, vendor_id, device_id);
                println!("      └─ Class: 0x{:02X}, Subclass: 0x{:02X}", class_id, subclass_id);
                
                if class_id == 0x01 && subclass_id == 0x06 {
                    println!("    [INFO] AHCI Storage Controller verified. Ready for filesystem mounting.");
                }
                
                if class_id == 0x02 {
                    println!("    [INFO] Network Interface Controller verified.");
                }
                
                if class_id == 0x0C && subclass_id == 0x03 {
                    println!("    [INFO] xHCI/eHCI USB Controller verified.");
                }
                println!("");
            }
        }
    }
    
    println!("🏁 PCI Bus Enumeration complete. Devices found: {}", found_devices);
    println!("===========================================\n");
}
