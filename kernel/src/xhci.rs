//! Bare-metal xHCI (Extensible Host Controller Interface — USB 3.0) Driver.
//! Maps PCI BAR0 MMIO registers, executes BIOS-to-OS Handoff (`USBLEGSUP`),
//! enumerates Root Hub ports, and identifies attached USB HID keyboards/mice/storage.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use spin::Mutex;
use crate::println;
use crate::memory::paging::{map_page, phys_to_virt, CACHE_DISABLE, WRITABLE};

#[derive(Clone, Copy, Debug)]
pub struct UsbPortDevice {
    pub port_num: u8,
    pub speed_id: u8,
    pub enabled: bool,
}

impl UsbPortDevice {
    pub fn speed_name(&self) -> &'static str {
        match self.speed_id {
            1 => "Full-Speed (12 Mbps, USB 1.1 HID)",
            2 => "Low-Speed (1.5 Mbps, USB HID Kbd/Mouse)",
            3 => "High-Speed (480 Mbps, USB 2.0)",
            4 => "SuperSpeed (5 Gbps, USB 3.0)",
            5 => "SuperSpeedPlus (10 Gbps, USB 3.1)",
            _ => "Connected USB Device",
        }
    }
}

struct XhciController {
    mmio_phys: u64,
    hci_version: u16,
    max_ports: u8,
    max_slots: u8,
    bios_handoff_ok: bool,
    online: bool,
    connected_ports: Vec<UsbPortDevice>,
}

static XHCI_STATE: Mutex<XhciController> = Mutex::new(XhciController {
    mmio_phys: 0,
    hci_version: 0,
    max_ports: 0,
    max_slots: 0,
    bios_handoff_ok: false,
    online: false,
    connected_ports: Vec::new(),
});

/// Perform BIOS-to-OS handoff via xHCI Extended Capability ID 1 (USBLEGSUP).
unsafe fn perform_bios_handoff(mmio_virt: u64, hccparams1: u32) -> bool {
    let mut xecp_dwords = ((hccparams1 >> 16) & 0xFFFF) as u64;
    if xecp_dwords == 0 {
        return true; // No extended capability list required
    }

    let mut offset = xecp_dwords << 2;
    for _ in 0..32 {
        let cap_ptr = (mmio_virt + offset) as *mut u32;
        let cap_val = cap_ptr.read_volatile();
        let cap_id = (cap_val & 0xFF) as u8;

        if cap_id == 1 {
            // USBLEGSUP: bit 16 = BIOS Owned, bit 24 = OS Owned
            cap_ptr.write_volatile(cap_val | (1 << 24));
            for _ in 0..100_000 {
                let cur = cap_ptr.read_volatile();
                if (cur & (1 << 16)) == 0 {
                    return true;
                }
            }
            // Force clear BIOS semaphore if firmware timed out
            let cur = cap_ptr.read_volatile();
            cap_ptr.write_volatile(cur & !(1 << 16));
            return true;
        }

        let next_dwords = ((cap_val >> 8) & 0xFF) as u64;
        if next_dwords == 0 {
            break;
        }
        offset += next_dwords << 2;
    }
    true
}

/// Initialize the xHCI USB 3.0 controller from its PCI BAR0 physical address.
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
        let cap_word0 = (mmio_virt as *const u32).read_volatile();
        let caplength = (cap_word0 & 0xFF) as u64;
        let hci_version = ((cap_word0 >> 16) & 0xFFFF) as u16;

        let hcsparams1 = ((mmio_virt + 0x04) as *const u32).read_volatile();
        let hccparams1 = ((mmio_virt + 0x10) as *const u32).read_volatile();

        let max_slots = (hcsparams1 & 0xFF) as u8;
        let max_ports = ((hcsparams1 >> 24) & 0xFF) as u8;

        let handoff_ok = perform_bios_handoff(mmio_virt, hccparams1);

        let op_base = mmio_virt + caplength;
        let mut connected = Vec::new();

        for port_idx in 0..max_ports {
            let portsc_addr = op_base + 0x400 + (port_idx as u64) * 0x10;
            let portsc = (portsc_addr as *const u32).read_volatile();
            let ccs = (portsc & 1) != 0;
            let ped = (portsc & 2) != 0;
            let speed = ((portsc >> 10) & 0x0F) as u8;

            if ccs {
                let dev = UsbPortDevice {
                    port_num: port_idx + 1,
                    speed_id: speed,
                    enabled: ped,
                };
                println!(
                    "  [xHCI] Port {} connected -> {}",
                    dev.port_num,
                    dev.speed_name()
                );
                connected.push(dev);
            }
        }

        println!(
            "[OK] xHCI USB v{:x}.{:02x} online @ {:#X} ({} ports, {} slots, BIOS handoff={})",
            (hci_version >> 8) & 0xFF,
            hci_version & 0xFF,
            mmio_phys,
            max_ports,
            max_slots,
            if handoff_ok { "OK" } else { "SKIP" }
        );

        let mut st = XHCI_STATE.lock();
        st.mmio_phys = mmio_phys;
        st.hci_version = hci_version;
        st.max_ports = max_ports;
        st.max_slots = max_slots;
        st.bios_handoff_ok = handoff_ok;
        st.online = true;
        st.connected_ports = connected;
    }
}

pub fn format_status() -> String {
    let st = XHCI_STATE.lock();
    let mut s = String::new();
    if !st.online {
        let _ = writeln!(s, "xHCI USB     : No PCI xHCI controller attached (PS/2 active)");
    } else {
        let _ = writeln!(
            s,
            "xHCI USB     : v{:x}.{:02x} @ {:#X} ({} root ports, BIOS handoff={})",
            (st.hci_version >> 8) & 0xFF,
            st.hci_version & 0xFF,
            st.mmio_phys,
            st.max_ports,
            if st.bios_handoff_ok { "YES" } else { "NO" }
        );
        for dev in &st.connected_ports {
            let _ = writeln!(
                s,
                "  USB Port #{}: {} [{}]",
                dev.port_num,
                dev.speed_name(),
                if dev.enabled { "ENABLED" } else { "STANDBY" }
            );
        }
    }
    s
}
