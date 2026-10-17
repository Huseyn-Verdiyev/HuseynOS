//! Unified Hardware Block Layer & GUID Partition Table (GPT) Engine.
//! Routes disk reads/writes transparently across NVMe DMA, AHCI SATA DMA, and ATA PIO,
//! and writes a standard UEFI GPT partition layout (ESP + HuseynOS Root Partition).

use alloc::string::String;
use core::fmt::Write;

pub const ESP_PART_START_LBA: u64 = 64;
pub const ESP_PART_SECTORS: u64 = 1984;
pub const HUSEYNOS_PART_START_LBA: u64 = 2048;

/// Check if any physical block storage device is online (NVMe, AHCI SATA, or ATA PIO).
pub fn has_physical_disk() -> bool {
    crate::nvme::is_available() || crate::ahci::is_available() || crate::ata::is_available()
}

/// Read a 512-byte sector from the active physical disk (NVMe preferred, then AHCI DMA, then ATA PIO).
pub fn read_disk_sector(lba: u64, buf: &mut [u8; 512]) -> bool {
    if crate::nvme::is_available() && crate::nvme::read_sector(lba, buf) {
        return true;
    }
    if crate::ahci::is_available() && crate::ahci::read_sector(lba, buf) {
        return true;
    }
    if lba <= 0x0FFF_FFFF && crate::ata::is_available() {
        return crate::ata::read_sector(lba as u32, buf);
    }
    false
}

/// Write a 512-byte sector to the active physical disk (NVMe preferred, then AHCI DMA, then ATA PIO).
pub fn write_disk_sector(lba: u64, buf: &[u8; 512]) -> bool {
    if crate::nvme::is_available() && crate::nvme::write_sector(lba, buf) {
        return true;
    }
    if crate::ahci::is_available() && crate::ahci::write_sector(lba, buf) {
        return true;
    }
    if lba <= 0x0FFF_FFFF && crate::ata::is_available() {
        return crate::ata::write_sector(lba as u32, buf);
    }
    false
}

/// Check if LBA 1 on the physical disk contains a valid GPT Header ("EFI PART") with HuseynOS installed.
pub fn is_gpt_installed() -> bool {
    if !has_physical_disk() {
        return false;
    }
    let mut sector = [0u8; 512];
    if !read_disk_sector(1, &mut sector) {
        return false;
    }
    &sector[0..8] == b"EFI PART" && &sector[56..64] == b"HUSEYNOS"
}

/// Write a Protective MBR (LBA 0), GPT Header (LBA 1), and GPT Partition Entries (LBA 2):
///   - Partition 1 (LBA 64..2047):   EFI System Partition (ESP)
///   - Partition 2 (LBA 2048..8191): HuseynOS Root Partition
pub fn write_gpt_layout() -> bool {
    if !has_physical_disk() {
        return false;
    }

    let mut mbr = [0u8; 512];
    mbr[446 + 4] = 0xEE;
    mbr[446 + 8..446 + 12].copy_from_slice(&1u32.to_le_bytes());
    mbr[446 + 12..446 + 16].copy_from_slice(&8191u32.to_le_bytes());
    mbr[510] = 0x55;
    mbr[511] = 0xAA;
    if !write_disk_sector(0, &mbr) {
        return false;
    }

    let mut gpt_hdr = [0u8; 512];
    gpt_hdr[0..8].copy_from_slice(b"EFI PART");
    gpt_hdr[8..12].copy_from_slice(&0x0001_0000u32.to_le_bytes());
    gpt_hdr[12..16].copy_from_slice(&92u32.to_le_bytes());
    gpt_hdr[24..32].copy_from_slice(&1u64.to_le_bytes());
    gpt_hdr[32..40].copy_from_slice(&8191u64.to_le_bytes());
    gpt_hdr[40..48].copy_from_slice(&64u64.to_le_bytes());
    gpt_hdr[48..56].copy_from_slice(&8150u64.to_le_bytes());
    gpt_hdr[56..64].copy_from_slice(b"HUSEYNOS");
    gpt_hdr[64..72].copy_from_slice(b"-MONOLIT");
    gpt_hdr[72..80].copy_from_slice(&2u64.to_le_bytes());
    gpt_hdr[80..84].copy_from_slice(&2u32.to_le_bytes());
    gpt_hdr[84..88].copy_from_slice(&128u32.to_le_bytes());
    if !write_disk_sector(1, &gpt_hdr) {
        return false;
    }

    let mut entries = [0u8; 512];

    let esp_guid: [u8; 16] = [
        0x28, 0x73, 0x2A, 0xC1, 0x1F, 0xF8, 0xD2, 0x11,
        0xBA, 0x4B, 0x00, 0xA0, 0xC9, 0x3E, 0xC9, 0x3B,
    ];
    entries[0..16].copy_from_slice(&esp_guid);
    entries[16..24].copy_from_slice(b"ESP-GUID");
    entries[32..40].copy_from_slice(&ESP_PART_START_LBA.to_le_bytes());
    entries[40..48].copy_from_slice(&(ESP_PART_START_LBA + ESP_PART_SECTORS - 1).to_le_bytes());

    let root_guid: [u8; 16] = [
        0xAF, 0x3D, 0xC6, 0x0F, 0x83, 0x84, 0x72, 0x47,
        0x8E, 0x79, 0x3D, 0x69, 0xD8, 0x47, 0x7D, 0xE4,
    ];
    entries[128..144].copy_from_slice(&root_guid);
    entries[144..152].copy_from_slice(b"HUSEYNFS");
    entries[160..168].copy_from_slice(&HUSEYNOS_PART_START_LBA.to_le_bytes());
    entries[168..176].copy_from_slice(&5120u64.to_le_bytes());

    if !write_disk_sector(2, &entries) {
        return false;
    }

    format_esp_partition()
}

fn format_esp_partition() -> bool {
    let mut bpb = [0u8; 512];
    bpb[0..3].copy_from_slice(&[0xEB, 0x3C, 0x90]);
    bpb[3..11].copy_from_slice(b"HUSEFI16");
    bpb[11..13].copy_from_slice(&512u16.to_le_bytes());
    bpb[13] = 1;
    bpb[14..16].copy_from_slice(&1u16.to_le_bytes());
    bpb[16] = 2;
    bpb[17..19].copy_from_slice(&64u16.to_le_bytes());
    bpb[19..21].copy_from_slice(&(ESP_PART_SECTORS as u16).to_le_bytes());
    bpb[21] = 0xF8;
    bpb[22..24].copy_from_slice(&6u16.to_le_bytes());
    bpb[36] = 0x80;
    bpb[38] = 0x29;
    bpb[39..43].copy_from_slice(&0x1234_5678u32.to_le_bytes());
    bpb[43..54].copy_from_slice(b"EFI SYSTEM ");
    bpb[54..62].copy_from_slice(b"FAT16   ");
    bpb[510] = 0x55;
    bpb[511] = 0xAA;

    if !write_disk_sector(ESP_PART_START_LBA, &bpb) {
        return false;
    }

    let mut fat = [0u8; 512];
    fat[0..4].copy_from_slice(&[0xF8, 0xFF, 0xFF, 0xFF]);
    let _ = write_disk_sector(ESP_PART_START_LBA + 1, &fat);
    let _ = write_disk_sector(ESP_PART_START_LBA + 7, &fat);

    let mut root_dir = [0u8; 512];
    root_dir[0..11].copy_from_slice(b"EFI SYSTEM ");
    root_dir[11] = 0x08;
    root_dir[32..43].copy_from_slice(b"EFI        ");
    root_dir[32 + 11] = 0x10;
    root_dir[32 + 26..32 + 28].copy_from_slice(&2u16.to_le_bytes());
    root_dir[64..75].copy_from_slice(b"BOOT       ");
    root_dir[64 + 11] = 0x10;
    root_dir[64 + 26..64 + 28].copy_from_slice(&3u16.to_le_bytes());

    write_disk_sector(ESP_PART_START_LBA + 13, &root_dir)
}

pub fn format_disk_report() -> String {
    let mut s = String::new();
    let backend = if crate::nvme::is_available() {
        "NVMe PCIe SSD DMA"
    } else if crate::ahci::is_available() {
        "AHCI SATA 3.0 DMA"
    } else if crate::ata::is_available() {
        "ATA/IDE 28-bit PIO"
    } else {
        "None (Live RAM-Disk Only)"
    };
    let gpt = is_gpt_installed();
    let _ = writeln!(s, "Primary Disk : {}", backend);
    let _ = writeln!(
        s,
        "GPT Table    : {}",
        if gpt { "INSTALLED (p1=ESP Bootloader @ LBA 64, p2=HUSEYNOS-ROOT @ LBA 2048)" } else { "NOT FORMATTED (Run 'install' to partition & install to disk)" }
    );
    let _ = writeln!(s, "Boot Source  : {}", crate::fat32::boot_source_name());
    s
}
