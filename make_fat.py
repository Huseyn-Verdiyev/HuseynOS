import struct
import sys
import os

def create_fat12_floppy(image_path, source_files):
    # 1.44 MB Floppy Disk parameters
    bytes_per_sector = 512
    sectors_per_cluster = 1
    reserved_sectors = 1
    number_of_fats = 2
    root_dir_entries = 224
    total_sectors = 2880
    media_descriptor = 0xF0
    sectors_per_fat = 9
    sectors_per_track = 18
    num_heads = 2
    hidden_sectors = 0
    total_sectors_32 = 0
    drive_number = 0
    reserved = 0
    boot_signature = 0x29
    volume_id = 0x12345678
    volume_label = b'HUSEYNFS   '
    fs_type = b'FAT12   '

    # Boot Sector (512 bytes)
    boot_sector = bytearray(512)
    boot_sector[0:3] = b'\xEB\x3C\x90'
    boot_sector[3:11] = b'MSWIN4.1'
    struct.pack_into('<HBHBHHBHHHII', boot_sector, 11,
        bytes_per_sector, sectors_per_cluster, reserved_sectors, number_of_fats,
        root_dir_entries, total_sectors, media_descriptor, sectors_per_fat,
        sectors_per_track, num_heads, hidden_sectors, total_sectors_32)
    struct.pack_into('<BBBI11s8s', boot_sector, 36,
        drive_number, reserved, boot_signature, volume_id, volume_label, fs_type)
    boot_sector[510:512] = b'\x55\xAA'

    # FAT Tables (2 * 9 sectors * 512 bytes = 9216 bytes)
    fat_size = sectors_per_fat * bytes_per_sector
    fat1 = bytearray(fat_size)
    fat1[0:3] = b'\xF0\xFF\xFF'

    # Root Directory (224 entries * 32 bytes = 7168 bytes = 14 sectors)
    root_dir_size = root_dir_entries * 32
    root_dir = bytearray(root_dir_size)
    
    # Volume label entry
    root_dir[0:11] = volume_label
    root_dir[11] = 0x08 # Volume ID attribute

    # Data Area
    data_area_size = (total_sectors - reserved_sectors - (number_of_fats * sectors_per_fat) - (root_dir_size // bytes_per_sector)) * bytes_per_sector
    data_area = bytearray(data_area_size)

    current_cluster = 2 # First usable cluster
    data_area_cursor = 0
    entry_offset = 32 # Skip volume label

    for source_file in source_files:
        with open(source_file, 'rb') as f:
            file_data = f.read()

        file_size = len(file_data)
        filename = os.path.basename(source_file).upper()
        name_part, ext_part = (filename.split('.') + [''])[:2]
        fat_filename = f"{name_part[:8]:<8}{ext_part[:3]:<3}".encode('ascii')

        clusters_needed = (file_size + bytes_per_sector - 1) // bytes_per_sector
        if clusters_needed == 0:
            clusters_needed = 1

        file_start_cluster = current_cluster

        # Fill FAT for the file
        for i in range(clusters_needed - 1):
            next_cluster = current_cluster + 1
            # Calculate byte offset in FAT
            offset = current_cluster + (current_cluster // 2)
            if current_cluster % 2 == 0:
                fat1[offset] = next_cluster & 0xFF
                fat1[offset + 1] = (fat1[offset + 1] & 0xF0) | ((next_cluster >> 8) & 0x0F)
            else:
                fat1[offset] = (fat1[offset] & 0x0F) | ((next_cluster << 4) & 0xF0)
                fat1[offset + 1] = (next_cluster >> 4) & 0xFF
            current_cluster = next_cluster

        # EOF marker (0xFFF) for the last cluster
        offset = current_cluster + (current_cluster // 2)
        if current_cluster % 2 == 0:
            fat1[offset] = 0xFF
            fat1[offset + 1] = (fat1[offset + 1] & 0xF0) | 0x0F
        else:
            fat1[offset] = (fat1[offset] & 0x0F) | 0xF0
            fat1[offset + 1] = 0xFF
            
        current_cluster += 1

        # File entry
        root_dir[entry_offset:entry_offset+11] = fat_filename
        root_dir[entry_offset+11] = 0x20 # Archive attribute
        struct.pack_into('<HHHI', root_dir, entry_offset + 22,
            0, 0, file_start_cluster, file_size)
        entry_offset += 32
        
        # Copy file data into data area
        data_area[data_area_cursor:data_area_cursor+file_size] = file_data
        data_area_cursor += (clusters_needed * bytes_per_sector)
        
        print(f"Injected {filename} ({file_size} bytes) at cluster {file_start_cluster}")

    fat2 = bytearray(fat1)

    # Write everything to the image file
    with open(image_path, 'wb') as f:
        f.write(boot_sector)
        f.write(fat1)
        f.write(fat2)
        f.write(root_dir)
        f.write(data_area)
    
    print(f"Successfully created FAT12 floppy image: {image_path}")

if __name__ == "__main__":
    if len(sys.argv) < 3:
        print("Usage: make_fat.py <output.img> <source_file1> [source_file2] ...")
        sys.exit(1)
    create_fat12_floppy(sys.argv[1], sys.argv[2:])
