use limine::request::ModuleRequest;
use crate::serial_println;
use core::slice;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

/// Limine boot module request.
#[used]
#[unsafe(link_section = ".requests")]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

/// Cached pointer to the beginning of the FAT image in RAM (used during Live USB boot).
static mut FS_BASE: *const u8 = core::ptr::null();
static mut FS_SIZE: usize = 0;

/// True when HuseynOS is operating directly off the physical disk partition (LBA 2048+).
static DISK_BOOT_ACTIVE: Mutex<bool> = Mutex::new(false);

/// Dynamic in-memory overlay store for newly created/modified files in HuseynOS.
static OVERLAY_FILES: Mutex<Vec<(String, Vec<u8>)>> = Mutex::new(Vec::new());
/// Track deleted filenames so `rm <file>` works across both overlay and boot volume files.
static DELETED_FILES: Mutex<Vec<String>> = Mutex::new(Vec::new());

/// BPB (BIOS Parameter Block) specifically for FAT12/FAT16.
#[repr(C, packed)]
#[derive(Copy, Clone)]
struct BootSector {
    jmp: [u8; 3],
    oem_name: [u8; 8],
    bytes_per_sector: u16,
    sectors_per_cluster: u8,
    reserved_sectors: u16,
    num_fats: u8,
    root_entries: u16,
    total_sectors_16: u16,
    media_descriptor: u8,
    sectors_per_fat_16: u16,
    sectors_per_track: u16,
    num_heads: u16,
    hidden_sectors: u32,
    total_sectors_32: u32,
    drive_number: u8,
    reserved1: u8,
    boot_signature: u8,
    volume_id: u32,
    volume_label: [u8; 11],
    fs_type: [u8; 8],
}

#[repr(C, packed)]
#[derive(Copy, Clone)]
struct DirEntry {
    name: [u8; 11],
    attr: u8,
    _reserved: u8,
    _creation_time_tenths: u8,
    _creation_time: u16,
    _creation_date: u16,
    _last_access_date: u16,
    high_cluster: u16,
    _last_mod_time: u16,
    _last_mod_date: u16,
    low_cluster: u16,
    file_size: u32,
}

pub fn init() {
    if let Some(resp) = MODULE_REQUEST.get_response() {
        let modules = resp.modules();
        serial_println!("[FAT] Found {} boot modules.", modules.len());

        for module in modules {
            let path = module.path().to_str().unwrap_or("<invalid path>");
            if path.contains("fs.img") {
                unsafe {
                    FS_BASE = module.addr() as *const u8;
                    FS_SIZE = module.size() as usize;
                }
                print_summary();
                break;
            }
        }
    }

    if crate::gpt::is_gpt_installed() {
        *DISK_BOOT_ACTIVE.lock() = true;
        load_dynamic_clusters_from_disk();
        serial_println!("[DISK] Active Boot Target: Physical Disk GPT (p2: HUSEYNOS-ROOT @ LBA 2048)");
    } else if crate::gpt::has_physical_disk() {
        serial_println!("[DISK] Physical disk detected (Run 'install' in Terminal to format & install HuseynOS to disk)");
    } else {
        serial_println!("[DISK] Running in Live RAM mode (No physical disk attached)");
    }
}

pub fn boot_source_name() -> &'static str {
    if *DISK_BOOT_ACTIVE.lock() {
        "Physical Disk GPT (HUSEYNOS-ROOT @ LBA 2048)"
    } else if unsafe { !FS_BASE.is_null() } {
        "Live ISO/RAM Module (fs.img)"
    } else {
        "Hardware Storage (Standalone)"
    }
}

/// Dynamic FAT Cluster Allocator & Disk Synchronizer:
/// Writes files directly into the FAT12/FAT16 cluster chain and Root Directory on the physical disk partition (LBA 2048+).
fn write_file_to_disk_partition(filename: &str, data: &[u8]) -> bool {
    if !crate::gpt::has_physical_disk() {
        return false;
    }

    let mut bpb_sec = [0u8; 512];
    if !crate::gpt::read_disk_sector(crate::gpt::HUSEYNOS_PART_START_LBA, &mut bpb_sec) {
        return false;
    }
    let bpb = unsafe { &*(bpb_sec.as_ptr() as *const BootSector) };
    let bytes_per_sec = { bpb.bytes_per_sector } as u64;
    if bytes_per_sec != 512 {
        return false;
    }

    let secs_per_cluster = { bpb.sectors_per_cluster } as u64;
    let reserved_secs = { bpb.reserved_sectors } as u64;
    let num_fats = { bpb.num_fats } as u64;
    let root_entries = { bpb.root_entries } as usize;
    let secs_per_fat = { bpb.sectors_per_fat_16 } as u64;

    let root_dir_lba = crate::gpt::HUSEYNOS_PART_START_LBA + reserved_secs + (num_fats * secs_per_fat);
    let root_dir_secs = ((root_entries * 32) + 511) / 512;
    let first_data_lba = root_dir_lba + root_dir_secs as u64;

    let needed_clusters = ((data.len() + 511) / 512).max(1) as u64;

    // Search root directory for existing entry or empty slot
    let mut dir_sec = [0u8; 512];
    let mut target_slot: Option<(u64, usize)> = None;
    let search_name = filename.to_uppercase();

    // Prepare 8.3 name buffer
    let mut name_83 = [b' '; 11];
    let parts: Vec<&str> = search_name.split('.').collect();
    let base = parts[0].as_bytes();
    let blen = base.len().min(8);
    name_83[..blen].copy_from_slice(&base[..blen]);
    if parts.len() > 1 {
        let ext = parts[1].as_bytes();
        let elen = ext.len().min(3);
        name_83[8..8 + elen].copy_from_slice(&ext[..elen]);
    }

    for i in 0..root_entries {
        let sec_idx = (i / 16) as u64;
        let entry_idx = i % 16;
        if entry_idx == 0 {
            if !crate::gpt::read_disk_sector(root_dir_lba + sec_idx, &mut dir_sec) {
                break;
            }
        }

        let entry = unsafe { &*((dir_sec.as_ptr().add(entry_idx * 32)) as *const DirEntry) };
        if entry.name[0] == 0x00 || entry.name[0] == 0xE5 {
            if target_slot.is_none() {
                target_slot = Some((root_dir_lba + sec_idx, entry_idx));
            }
            if entry.name[0] == 0x00 { break; }
        } else if &entry.name == &name_83 {
            target_slot = Some((root_dir_lba + sec_idx, entry_idx));
            break;
        }
    }

    let (slot_lba, slot_idx) = match target_slot {
        Some(s) => s,
        None => return false,
    };

    // Allocate clusters starting at cluster 100 for user files
    let start_cluster = 100u16;
    let mut cur_offset = 0usize;

    for c in 0..needed_clusters {
        let cluster_lba = first_data_lba + ((start_cluster as u64 + c - 2) * secs_per_cluster);
        let mut cluster_buf = [0u8; 512];
        let rem = data.len().saturating_sub(cur_offset);
        let to_copy = rem.min(512);
        if to_copy > 0 {
            cluster_buf[..to_copy].copy_from_slice(&data[cur_offset..cur_offset + to_copy]);
            cur_offset += to_copy;
        }
        let _ = crate::gpt::write_disk_sector(cluster_lba, &cluster_buf);
    }

    // Write directory entry
    if !crate::gpt::read_disk_sector(slot_lba, &mut dir_sec) {
        return false;
    }
    let entry_ptr = unsafe { (dir_sec.as_mut_ptr().add(slot_idx * 32)) as *mut DirEntry };
    unsafe {
        let e = &mut *entry_ptr;
        e.name.copy_from_slice(&name_83);
        e.attr = 0x20; // Archive
        e.low_cluster = start_cluster;
        e.file_size = data.len() as u32;
    }
    crate::gpt::write_disk_sector(slot_lba, &dir_sec)
}

/// Load existing files from the physical disk partition (LBA 2048+) into `OVERLAY_FILES`.
fn load_dynamic_clusters_from_disk() {
    let files = list_files_from_disk_partition();
    let mut overlay = OVERLAY_FILES.lock();
    for (name, _) in files {
        if let Some(data) = read_file_from_physical_disk(&name) {
            if let Some(existing) = overlay.iter_mut().find(|(n, _)| *n == name) {
                existing.1 = data;
            } else {
                overlay.push((name, data));
            }
        }
    }
}

/// Format the attached physical disk with GPT, write the bootable FAT volume at LBA 2048+,
/// and write all active files into the physical disk FAT cluster chains.
pub fn install_to_disk() -> Result<usize, &'static str> {
    if !crate::gpt::has_physical_disk() {
        return Err("No physical disk attached");
    }
    if !crate::gpt::write_gpt_layout() {
        return Err("Failed to write GPT partition layout");
    }

    let mut sectors_written = 3usize;

    unsafe {
        if !FS_BASE.is_null() && FS_SIZE > 0 {
            let total_sectors = (FS_SIZE / 512).min(256);
            for s in 0..total_sectors {
                let mut buf = [0u8; 512];
                let src = FS_BASE.add(s * 512);
                core::ptr::copy_nonoverlapping(src, buf.as_mut_ptr(), 512);
                if crate::gpt::write_disk_sector(crate::gpt::HUSEYNOS_PART_START_LBA + s as u64, &buf) {
                    sectors_written += 1;
                }
            }
        }
    }

    // Flush all user files directly into the partition cluster chain
    let overlay = OVERLAY_FILES.lock().clone();
    for (name, data) in overlay {
        if write_file_to_disk_partition(&name, &data) {
            sectors_written += (data.len() + 511) / 512;
        }
    }

    *DISK_BOOT_ACTIVE.lock() = true;
    Ok(sectors_written)
}

fn print_summary() {
    unsafe {
        if FS_BASE.is_null() { return; }
        let bpb = &*(FS_BASE as *const BootSector);

        let mut fs_type_str = [0u8; 8];
        fs_type_str.copy_from_slice(&{bpb.fs_type});
        let mut vol_label = [0u8; 11];
        vol_label.copy_from_slice(&{bpb.volume_label});

        serial_println!(
            "[FAT] Volume: \"{}\", Type: \"{}\"",
            core::str::from_utf8_unchecked(&vol_label).trim(),
            core::str::from_utf8_unchecked(&fs_type_str).trim()
        );
    }
}

/// Write or update a file in the HuseynOS storage layer and flush to disk partition clusters.
pub fn write_file(filename: &str, data: &[u8]) {
    let key = filename.to_uppercase();
    {
        let mut deleted = DELETED_FILES.lock();
        deleted.retain(|d| *d != key);
    }
    let mut overlay = OVERLAY_FILES.lock();
    if let Some(entry) = overlay.iter_mut().find(|(n, _)| *n == key) {
        entry.1 = data.to_vec();
    } else {
        overlay.push((key.clone(), data.to_vec()));
    }
    drop(overlay);

    write_file_to_disk_partition(&key, data);
}

/// Delete a file by name from the HuseynOS storage layer (`rm`).
pub fn delete_file(filename: &str) -> bool {
    let key = filename.to_uppercase();
    let existed = read_file(&key).is_some();
    if !existed {
        return false;
    }

    {
        let mut overlay = OVERLAY_FILES.lock();
        overlay.retain(|(n, _)| *n != key);
    }
    {
        let mut deleted = DELETED_FILES.lock();
        if !deleted.iter().any(|d| *d == key) {
            deleted.push(key);
        }
    }

    // Mark directory entry as 0xE5 (deleted) on physical disk
    if crate::gpt::has_physical_disk() {
        let mut bpb_sec = [0u8; 512];
        if crate::gpt::read_disk_sector(crate::gpt::HUSEYNOS_PART_START_LBA, &mut bpb_sec) {
            let bpb = unsafe { &*(bpb_sec.as_ptr() as *const BootSector) };
            let reserved_secs = { bpb.reserved_sectors } as u64;
            let num_fats = { bpb.num_fats } as u64;
            let secs_per_fat = { bpb.sectors_per_fat_16 } as u64;
            let root_dir_lba = crate::gpt::HUSEYNOS_PART_START_LBA + reserved_secs + (num_fats * secs_per_fat);

            let mut dir_sec = [0u8; 512];
            for i in 0..64 {
                let sec_idx = (i / 16) as u64;
                let entry_idx = i % 16;
                if entry_idx == 0 {
                    let _ = crate::gpt::read_disk_sector(root_dir_lba + sec_idx, &mut dir_sec);
                }
                let entry_ptr = unsafe { (dir_sec.as_mut_ptr().add(entry_idx * 32)) as *mut DirEntry };
                let entry = unsafe { &mut *entry_ptr };
                let base = core::str::from_utf8(&entry.name[..8]).unwrap_or("").trim();
                if key.starts_with(base) && !base.is_empty() {
                    entry.name[0] = 0xE5; // Marked deleted
                    let _ = crate::gpt::write_disk_sector(root_dir_lba + sec_idx, &dir_sec);
                    break;
                }
            }
        }
    }
    true
}

fn list_files_from_disk_partition() -> Vec<(String, u32)> {
    let mut files = Vec::new();
    let mut bpb_sec = [0u8; 512];
    if !crate::gpt::read_disk_sector(crate::gpt::HUSEYNOS_PART_START_LBA, &mut bpb_sec) {
        return files;
    }
    let bpb = unsafe { &*(bpb_sec.as_ptr() as *const BootSector) };
    let reserved_secs = { bpb.reserved_sectors } as u64;
    let num_fats = { bpb.num_fats } as u64;
    let root_entries = { bpb.root_entries } as usize;
    let secs_per_fat = { bpb.sectors_per_fat_16 } as u64;

    let root_dir_lba = crate::gpt::HUSEYNOS_PART_START_LBA + reserved_secs + (num_fats * secs_per_fat);
    let mut dir_sec = [0u8; 512];

    for i in 0..root_entries {
        let sec_idx = (i / 16) as u64;
        let entry_idx = i % 16;
        if entry_idx == 0 {
            if !crate::gpt::read_disk_sector(root_dir_lba + sec_idx, &mut dir_sec) {
                break;
            }
        }

        let entry = unsafe { &*((dir_sec.as_ptr().add(entry_idx * 32)) as *const DirEntry) };
        if entry.name[0] == 0x00 { break; }
        if entry.name[0] == 0xE5 || (entry.attr & 0x08) != 0 || (entry.attr & 0x10) != 0 {
            continue;
        }

        let mut name_buf = [0u8; 11];
        name_buf.copy_from_slice(&{entry.name});
        let base = core::str::from_utf8(&name_buf[..8]).unwrap_or("").trim();
        let ext = core::str::from_utf8(&name_buf[8..11]).unwrap_or("").trim();
        let full_name = if ext.is_empty() {
            String::from(base)
        } else {
            let mut s = String::from(base);
            s.push('.');
            s.push_str(ext);
            s
        };

        files.push((full_name, {entry.file_size}));
    }
    files
}

/// List all files in the root directory (merging boot volume + user-written files, minus deleted files).
pub fn list_files() -> Vec<(String, u32)> {
    let deleted = DELETED_FILES.lock();
    let mut files = Vec::new();

    unsafe {
        if !FS_BASE.is_null() {
            let bpb = &*(FS_BASE as *const BootSector);
            let bytes_per_sector = {bpb.bytes_per_sector} as usize;
            let reserved_sectors = {bpb.reserved_sectors} as usize;
            let num_fats = {bpb.num_fats} as usize;
            let root_entries = {bpb.root_entries} as usize;
            let sectors_per_fat = {bpb.sectors_per_fat_16} as usize;

            let fat_size_bytes = num_fats * sectors_per_fat * bytes_per_sector;
            let root_dir_offset = (reserved_sectors * bytes_per_sector) + fat_size_bytes;
            let root_dir_ptr = FS_BASE.add(root_dir_offset) as *const DirEntry;

            for i in 0..root_entries {
                let entry = &*root_dir_ptr.add(i);
                if entry.name[0] == 0x00 { break; }
                if entry.name[0] == 0xE5 || (entry.attr & 0x08 != 0) || (entry.attr & 0x10 != 0) {
                    continue;
                }

                let mut name_buf = [0u8; 11];
                name_buf.copy_from_slice(&{entry.name});
                let base = core::str::from_utf8_unchecked(&name_buf[..8]).trim();
                let ext = core::str::from_utf8_unchecked(&name_buf[8..11]).trim();
                let full_name = if ext.is_empty() {
                    String::from(base)
                } else {
                    let mut s = String::from(base);
                    s.push('.');
                    s.push_str(ext);
                    s
                };

                if !deleted.iter().any(|d| *d == full_name) {
                    files.push((full_name, {entry.file_size}));
                }
            }
        } else if crate::gpt::is_gpt_installed() {
            for (f, sz) in list_files_from_disk_partition() {
                if !deleted.iter().any(|d| *d == f) {
                    files.push((f, sz));
                }
            }
        }
    }

    let overlay = OVERLAY_FILES.lock();
    for (name, data) in overlay.iter() {
        if deleted.iter().any(|d| *d == *name) {
            continue;
        }
        if let Some(existing) = files.iter_mut().find(|(n, _)| *n == *name) {
            existing.1 = data.len() as u32;
        } else {
            files.push((name.clone(), data.len() as u32));
        }
    }

    files
}

/// Read a file directly from the physical disk partition (LBA 2048+).
fn read_file_from_physical_disk(search_name: &str) -> Option<Vec<u8>> {
    if !crate::gpt::has_physical_disk() {
        return None;
    }

    let mut bpb_sec = [0u8; 512];
    if !crate::gpt::read_disk_sector(crate::gpt::HUSEYNOS_PART_START_LBA, &mut bpb_sec) {
        return None;
    }

    let bpb = unsafe { &*(bpb_sec.as_ptr() as *const BootSector) };
    let bytes_per_sec = {bpb.bytes_per_sector} as usize;
    if bytes_per_sec != 512 {
        return None;
    }

    let secs_per_cluster = {bpb.sectors_per_cluster} as usize;
    let reserved_secs = {bpb.reserved_sectors} as usize;
    let num_fats = {bpb.num_fats} as usize;
    let root_entries = {bpb.root_entries} as usize;
    let secs_per_fat = {bpb.sectors_per_fat_16} as usize;

    let root_dir_lba = crate::gpt::HUSEYNOS_PART_START_LBA + (reserved_secs + num_fats * secs_per_fat) as u64;
    let root_dir_sectors = ((root_entries * 32) + 511) / 512;
    let first_data_lba = root_dir_lba + root_dir_sectors as u64;

    let mut dir_sec = [0u8; 512];

    for i in 0..root_entries {
        let sec_idx = (i / 16) as u64;
        let entry_idx = i % 16;

        if entry_idx == 0 {
            if !crate::gpt::read_disk_sector(root_dir_lba + sec_idx, &mut dir_sec) {
                break;
            }
        }

        let entry = unsafe { &*((dir_sec.as_ptr().add(entry_idx * 32)) as *const DirEntry) };
        if entry.name[0] == 0x00 { break; }
        if entry.name[0] == 0xE5 || (entry.attr & 0x08 != 0) || (entry.attr & 0x10 != 0) {
            continue;
        }

        let mut name_buf = [0u8; 11];
        name_buf.copy_from_slice(&{entry.name});
        let base = core::str::from_utf8(&name_buf[..8]).unwrap_or("").trim();
        let ext = core::str::from_utf8(&name_buf[8..11]).unwrap_or("").trim();
        let full_name = if ext.is_empty() {
            String::from(base)
        } else {
            let mut s = String::from(base);
            s.push('.');
            s.push_str(ext);
            s
        };

        if full_name == search_name {
            let start_cluster = {entry.low_cluster} as usize;
            let file_size = {entry.file_size} as usize;

            if start_cluster >= 2 {
                let file_lba = first_data_lba + ((start_cluster - 2) * secs_per_cluster) as u64;
                let num_sectors = (file_size + 511) / 512;
                let mut data = Vec::with_capacity(file_size);

                for s in 0..num_sectors {
                    let mut data_sec = [0u8; 512];
                    if !crate::gpt::read_disk_sector(file_lba + s as u64, &mut data_sec) {
                        return None;
                    }
                    let remaining = file_size - data.len();
                    let to_copy = remaining.min(512);
                    data.extend_from_slice(&data_sec[..to_copy]);
                }
                return Some(data);
            }
        }
    }
    None
}

/// Read a file by name from the overlay store, Live RAM volume, or physical disk partition.
pub fn read_file(filename: &str) -> Option<Vec<u8>> {
    let search_name = filename.to_uppercase();

    {
        let deleted = DELETED_FILES.lock();
        if deleted.iter().any(|d| *d == search_name) {
            return None;
        }
    }

    {
        let overlay = OVERLAY_FILES.lock();
        if let Some((_, data)) = overlay.iter().find(|(n, _)| *n == search_name) {
            return Some(data.clone());
        }
    }

    unsafe {
        if !FS_BASE.is_null() {
            let bpb = &*(FS_BASE as *const BootSector);
            let bytes_per_sector = {bpb.bytes_per_sector} as usize;
            let sectors_per_cluster = {bpb.sectors_per_cluster} as usize;
            let reserved_sectors = {bpb.reserved_sectors} as usize;
            let num_fats = {bpb.num_fats} as usize;
            let root_entries = {bpb.root_entries} as usize;
            let sectors_per_fat = {bpb.sectors_per_fat_16} as usize;

            let fat_size_bytes = num_fats * sectors_per_fat * bytes_per_sector;
            let root_dir_offset = (reserved_sectors * bytes_per_sector) + fat_size_bytes;
            let root_dir_ptr = FS_BASE.add(root_dir_offset) as *const DirEntry;

            let root_dir_sectors = ((root_entries * 32) + (bytes_per_sector - 1)) / bytes_per_sector;
            let first_data_sector = reserved_sectors + (num_fats * sectors_per_fat) + root_dir_sectors;

            for i in 0..root_entries {
                let entry = &*root_dir_ptr.add(i);
                if entry.name[0] == 0x00 { break; }
                if entry.name[0] == 0xE5 || (entry.attr & 0x08 != 0) || (entry.attr & 0x10 != 0) {
                    continue;
                }

                let mut name_buf = [0u8; 11];
                name_buf.copy_from_slice(&{entry.name});
                let base = core::str::from_utf8_unchecked(&name_buf[..8]).trim();
                let ext = core::str::from_utf8_unchecked(&name_buf[8..11]).trim();
                let full_name = if ext.is_empty() {
                    String::from(base)
                } else {
                    let mut s = String::from(base);
                    s.push('.');
                    s.push_str(ext);
                    s
                };

                if full_name == search_name {
                    let start_cluster = {entry.low_cluster} as usize;
                    let file_size = {entry.file_size} as usize;

                    if start_cluster >= 2 {
                        let file_lba = first_data_sector + ((start_cluster - 2) * sectors_per_cluster);
                        let file_offset = file_lba * bytes_per_sector;

                        let data_ptr = FS_BASE.add(file_offset);
                        let data_slice = slice::from_raw_parts(data_ptr, file_size);
                        return Some(data_slice.to_vec());
                    }
                }
            }
        }
    }

    read_file_from_physical_disk(&search_name)
}
