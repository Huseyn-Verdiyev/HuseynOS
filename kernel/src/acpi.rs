//! Bare-metal ACPI (Advanced Configuration and Power Interface) RSDP, RSDT/XSDT, and MADT Parser.
//! Discovers hardware Local APIC base address, CPU cores (SMP topology), IOAPICs, and IRQ overrides.

use alloc::string::String;
use alloc::vec::Vec;
use core::fmt::Write;
use limine::request::RsdpRequest;
use spin::Mutex;
use crate::println;
use crate::memory::paging::phys_to_virt;

#[used]
#[unsafe(link_section = ".requests")]
static RSDP_REQUEST: RsdpRequest = RsdpRequest::new();

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct RsdpDescriptor {
    signature: [u8; 8],
    checksum: u8,
    oem_id: [u8; 6],
    revision: u8,
    rsdt_address: u32,
    length: u32,
    xsdt_address: u64,
    extended_checksum: u8,
    reserved: [u8; 3],
}

#[repr(C, packed)]
#[derive(Clone, Copy)]
struct AcpiSdtHeader {
    signature: [u8; 4],
    length: u32,
    revision: u8,
    checksum: u8,
    oem_id: [u8; 6],
    oem_table_id: [u8; 8],
    oem_revision: u32,
    creator_id: u32,
    creator_revision: u32,
}

#[derive(Clone, Copy, Debug)]
pub struct IoApicInfo {
    pub id: u8,
    pub address: u32,
    pub gsi_base: u32,
}

pub struct AcpiTopology {
    pub rsdp_revision: u8,
    pub lapic_addr: u64,
    pub cpu_apic_ids: Vec<u8>,
    pub ioapics: Vec<IoApicInfo>,
    pub iso_count: usize,
}

impl AcpiTopology {
    const fn new() -> Self {
        Self {
            rsdp_revision: 0,
            lapic_addr: 0xFEE0_0000,
            cpu_apic_ids: Vec::new(),
            ioapics: Vec::new(),
            iso_count: 0,
        }
    }
}

static ACPI_INFO: Mutex<AcpiTopology> = Mutex::new(AcpiTopology::new());

unsafe fn parse_madt(madt_virt: u64) {
    let hdr = &*(madt_virt as *const AcpiSdtHeader);
    let total_len = { hdr.length } as usize;
    if total_len < core::mem::size_of::<AcpiSdtHeader>() + 8 {
        return;
    }

    let lapic_addr32 = *((madt_virt + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32);
    let mut info = ACPI_INFO.lock();
    info.lapic_addr = lapic_addr32 as u64;

    let mut offset = core::mem::size_of::<AcpiSdtHeader>() + 8;
    let base_ptr = madt_virt as *const u8;

    while offset + 2 <= total_len {
        let entry_type = *base_ptr.add(offset);
        let entry_len = *base_ptr.add(offset + 1) as usize;
        if entry_len < 2 || offset + entry_len > total_len {
            break;
        }

        match entry_type {
            0 => {
                // Processor Local APIC
                if entry_len >= 8 {
                    let apic_id = *base_ptr.add(offset + 3);
                    let flags = *(base_ptr.add(offset + 4) as *const u32);
                    if (flags & 1) != 0 {
                        info.cpu_apic_ids.push(apic_id);
                    }
                }
            }
            1 => {
                // I/O APIC
                if entry_len >= 12 {
                    let id = *base_ptr.add(offset + 2);
                    let addr = (base_ptr.add(offset + 4) as *const u32).read_unaligned();
                    let gsi_base = (base_ptr.add(offset + 8) as *const u32).read_unaligned();
                    info.ioapics.push(IoApicInfo { id, address: addr, gsi_base });
                }
            }
            2 => {
                // Interrupt Source Override
                info.iso_count += 1;
            }
            5 => {
                // 64-bit Local APIC Address Override
                if entry_len >= 12 {
                    let addr64 = (base_ptr.add(offset + 4) as *const u64).read_unaligned();
                    info.lapic_addr = addr64;
                }
            }
            _ => {}
        }

        offset += entry_len;
    }
}

fn ensure_mapped(phys: u64, size: usize) -> u64 {
    let start_page = phys & !0xFFF;
    let end_page = (phys + size as u64 + 0xFFF) & !0xFFF;
    for page in (start_page..end_page).step_by(0x1000) {
        let virt = phys_to_virt(page);
        if crate::memory::paging::get_mapped_frame(crate::memory::paging::get_pml4(), virt).is_none() {
            crate::memory::paging::map_page(virt, page, crate::memory::paging::WRITABLE);
        }
    }
    phys_to_virt(phys)
}

pub fn init() {
    let resp = match RSDP_REQUEST.get_response() {
        Some(r) => r,
        None => {
            println!("[ACPI] No RSDP provided by bootloader (using defaults)");
            return;
        }
    };

    let rsdp_addr = resp.address() as u64;
    crate::serial_println!("[ACPI] RSDP raw address: {:#X}", rsdp_addr);
    let rsdp_virt = if rsdp_addr < 0xFFFF_8000_0000_0000 {
        ensure_mapped(rsdp_addr, core::mem::size_of::<RsdpDescriptor>())
    } else {
        rsdp_addr
    };
    crate::serial_println!("[ACPI] RSDP virt address: {:#X}", rsdp_virt);

    unsafe {
        let rsdp = &*(rsdp_virt as *const RsdpDescriptor);
        if &rsdp.signature != b"RSD PTR " {
            println!("[ACPI] Invalid RSDP signature");
            return;
        }
        let rev = rsdp.revision;
        crate::serial_println!("[ACPI] Signature OK! Rev: {}", rev);
        ACPI_INFO.lock().rsdp_revision = rev;

        if rev >= 2 && { rsdp.xsdt_address } != 0 {
            let xsdt_phys = { rsdp.xsdt_address };
            let xsdt_virt = ensure_mapped(xsdt_phys, 4096);
            let hdr = &*(xsdt_virt as *const AcpiSdtHeader);
            let total_len = { hdr.length } as usize;
            let xsdt_virt = ensure_mapped(xsdt_phys, total_len);
            let entries = total_len.saturating_sub(core::mem::size_of::<AcpiSdtHeader>()) / 8;
            let table_ptrs = (xsdt_virt + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u64;

            for i in 0..entries {
                let sdt_phys = table_ptrs.add(i).read_unaligned();
                let sdt_virt = ensure_mapped(sdt_phys, 4096);
                let sdt_hdr = &*(sdt_virt as *const AcpiSdtHeader);
                let table_len = { sdt_hdr.length } as usize;
                let sdt_virt = ensure_mapped(sdt_phys, table_len);
                let sdt_hdr = &*(sdt_virt as *const AcpiSdtHeader);
                if &sdt_hdr.signature == b"APIC" {
                    parse_madt(sdt_virt);
                }
            }
        } else if { rsdp.rsdt_address } != 0 {
            let rsdt_phys = { rsdp.rsdt_address } as u64;
            let rsdt_virt = ensure_mapped(rsdt_phys, 4096);
            let hdr = &*(rsdt_virt as *const AcpiSdtHeader);
            let total_len = { hdr.length } as usize;
            let rsdt_virt = ensure_mapped(rsdt_phys, total_len);
            let entries = total_len.saturating_sub(core::mem::size_of::<AcpiSdtHeader>()) / 4;
            let table_ptrs = (rsdt_virt + core::mem::size_of::<AcpiSdtHeader>() as u64) as *const u32;

            for i in 0..entries {
                let sdt_phys = table_ptrs.add(i).read_unaligned() as u64;
                let sdt_virt = ensure_mapped(sdt_phys, 4096);
                let sdt_hdr = &*(sdt_virt as *const AcpiSdtHeader);
                let table_len = { sdt_hdr.length } as usize;
                let sdt_virt = ensure_mapped(sdt_phys, table_len);
                let sdt_hdr = &*(sdt_virt as *const AcpiSdtHeader);
                if &sdt_hdr.signature == b"APIC" {
                    parse_madt(sdt_virt);
                }
            }
        }
    }

    let info = ACPI_INFO.lock();
    println!(
        "[OK] ACPI v{} MADT parsed: {} CPU core(s), LAPIC @ {:#X}, {} IOAPIC(s)",
        info.rsdp_revision,
        info.cpu_apic_ids.len().max(1),
        info.lapic_addr,
        info.ioapics.len()
    );
}

pub fn get_lapic_And_ioapic() -> (u64, Option<IoApicInfo>, usize) {
    let info = ACPI_INFO.lock();
    let first_ioapic = info.ioapics.first().copied();
    (info.lapic_addr, first_ioapic, info.cpu_apic_ids.len().max(1))
}

pub fn format_status() -> String {
    let info = ACPI_INFO.lock();
    let mut s = String::new();
    let _ = writeln!(
        s,
        "ACPI Rev     : v{} (ISO overrides: {})",
        info.rsdp_revision, info.iso_count
    );
    let _ = writeln!(
        s,
        "CPU Topology : {} hardware core(s) detected {:?}",
        info.cpu_apic_ids.len().max(1),
        info.cpu_apic_ids
    );
    let _ = writeln!(s, "Local APIC   : {:#010X}", info.lapic_addr);
    for io in &info.ioapics {
        let _ = writeln!(
            s,
            "I/O APIC #{}  : {:#010X} (GSI base {})",
            io.id, io.address, io.gsi_base
        );
    }
    s
}
