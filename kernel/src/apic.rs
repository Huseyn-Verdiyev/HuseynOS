//! Local APIC & I/O APIC Hardware Controller Driver.
//! Maps the Local APIC and IOAPIC MMIO pages with CACHE_DISABLE and initializes the Local APIC SVR.

use alloc::string::String;
use core::fmt::Write;
use spin::Mutex;
use crate::println;
use crate::memory::paging::{map_page, phys_to_virt, CACHE_DISABLE, WRITABLE};

struct ApicState {
    lapic_phys: u64,
    lapic_id: u8,
    lapic_ver: u8,
    ioapic_phys: u64,
    ioapic_max_redir: u8,
    cores: usize,
    active: bool,
}

static APIC_STATE: Mutex<ApicState> = Mutex::new(ApicState {
    lapic_phys: 0xFEE0_0000,
    lapic_id: 0,
    lapic_ver: 0,
    ioapic_phys: 0xFEC0_0000,
    ioapic_max_redir: 0,
    cores: 1,
    active: false,
});

/// Initialize Local APIC and probe IOAPIC redirection table capacity without breaking active timer IRQs.
pub fn init() {
    let (lapic_phys, ioapic_opt, cores) = crate::acpi::get_lapic_And_ioapic();
    let lapic_virt = phys_to_virt(lapic_phys);

    map_page(lapic_virt, lapic_phys, WRITABLE | CACHE_DISABLE);

    unsafe {
        let id_reg = ((lapic_virt + 0x020) as *const u32).read_volatile();
        let ver_reg = ((lapic_virt + 0x030) as *const u32).read_volatile();
        let svr_ptr = (lapic_virt + 0x0F0) as *mut u32;

        // Enable Local APIC (bit 8) with Spurious Vector 0xFF
        let svr = svr_ptr.read_volatile();
        svr_ptr.write_volatile(svr | 0x1FF);

        let lapic_id = ((id_reg >> 24) & 0xFF) as u8;
        let lapic_ver = (ver_reg & 0xFF) as u8;

        let mut max_redir = 23u8;
        let mut io_phys = 0xFEC0_0000u64;

        if let Some(io) = ioapic_opt {
            io_phys = io.address as u64;
            let io_virt = phys_to_virt(io_phys);
            map_page(io_virt, io_phys, WRITABLE | CACHE_DISABLE);

            let ioregsel = io_virt as *mut u32;
            let iowin = (io_virt + 0x10) as *const u32;
            ioregsel.write_volatile(0x01); // IOAPICVER register
            let io_ver = iowin.read_volatile();
            max_redir = ((io_ver >> 16) & 0xFF) as u8;
        }

        let mut st = APIC_STATE.lock();
        st.lapic_phys = lapic_phys;
        st.lapic_id = lapic_id;
        st.lapic_ver = lapic_ver;
        st.ioapic_phys = io_phys;
        st.ioapic_max_redir = max_redir;
        st.cores = cores;
        st.active = true;

        println!(
            "[OK] Local APIC #{} (v{:#x}) enabled; IOAPIC @ {:#X} ({} IRQ lines)",
            lapic_id,
            lapic_ver,
            io_phys,
            max_redir + 1
        );
    }
}

pub fn format_status() -> String {
    let st = APIC_STATE.lock();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "APIC Status  : {} (BSP LAPIC ID {}, v{:#x}, {} IRQ pins)",
        if st.active { "ONLINE" } else { "LEGACY-PIC" },
        st.lapic_id,
        st.lapic_ver,
        st.ioapic_max_redir + 1
    );
    s.push_str(&crate::acpi::format_status());
    s
}
