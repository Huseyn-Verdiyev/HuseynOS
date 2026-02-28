use limine::request::ModuleRequest;
use crate::{serial_print, serial_println};
use core::slice;
use alloc::string::String;
use alloc::vec::Vec;

/// Limine boot module request.
#[used]
#[unsafe(link_section = ".requests")]
static MODULE_REQUEST: ModuleRequest = ModuleRequest::new();

/// Cached pointer to the beginning of the FAT image in RAM.
static mut FS_BASE: *const u8 = core::ptr::null();
static mut FS_SIZE: usize = 0;

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
    // FAT12/16 specific Extended BPB
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
    let response = match MODULE_REQUEST.get_response() {
        Some(resp) => resp,
        None => {
            serial_println!("[FAIL] No modules provided by Limine.");
            return;
        }
    };

    let modules = response.modules();
    serial_println!("[FAT] Found {} boot modules.", modules.len());

    for module in modules {
        let path = module.path().to_str().unwrap_or("<invalid path>");
        if path.contains("fs.img") {
            serial_println!("[FAT] Mounted filesystem image ({} bytes)", module.size());
            unsafe {
                FS_BASE = module.addr() as *const u8;
                FS_SIZE = module.size() as usize;
            }
            // Print summary
            print_summary();
            return;
        }
    }
    serial_println!("[FAIL] fs.img not found in boot modules.");
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

/// List all files in the root directory. Returns a Vec of (name, size) tuples.
pub fn list_files() -> Vec<(String, u32)> {
    let mut files = Vec::new();
    unsafe {
        if FS_BASE.is_null() { return files; }
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
            if entry.name[0] == 0xE5 { continue; }
            if entry.attr & 0x08 != 0 { continue; } // Volume Label
            if entry.attr & 0x10 != 0 { continue; } // Directory

            let mut name_buf = [0u8; 11];
            name_buf.copy_from_slice(&{entry.name});

            // Format 8.3 name nicely
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

            files.push((full_name, {entry.file_size}));
        }
    }
    files
}

/// Read a file by name from the root directory. Returns file contents or None.
pub fn read_file(filename: &str) -> Option<Vec<u8>> {
    unsafe {
        if FS_BASE.is_null() { return None; }
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

        // Convert filename to uppercase 8.3 format for comparison
        let search_name = filename.to_uppercase();

        for i in 0..root_entries {
            let entry = &*root_dir_ptr.add(i);
            if entry.name[0] == 0x00 { break; }
            if entry.name[0] == 0xE5 { continue; }
            if entry.attr & 0x08 != 0 { continue; }
            if entry.attr & 0x10 != 0 { continue; }

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
        None
    }
}
