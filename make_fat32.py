import sys
import os
from pyfatfs.PyFat import PyFat

def create_fat32_image(image_path, source_file):
    # Create an empty 32MB file for the image
    img_size = 32 * 1024 * 1024
    with open(image_path, "wb") as f:
        f.seek(img_size - 1)
        f.write(b'\0')
    
    print(f"Created empty image: {image_path} ({img_size} bytes)")

    # Format the image as FAT32
    pf = PyFat()
    pf.mkfs(image_path, fat_type=32, label="HUSEYNFS")
    print("Formatted as FAT32.")

    # Open PyFat properly to write files
    pf.open(image_path)
    root_dir = pf.root_dir

    filename = os.path.basename(source_file)
    print(f"Copying {filename}...")

    # Read from source
    with open(source_file, 'rb') as src:
        data = src.read()
        
    # Write to FAT32
    # PyFatFS has add_file
    new_entry = root_dir.register_file(filename)
    new_entry.write(data)
    
    pf.close()
    print("FAT32 image building complete.")

if __name__ == "__main__":
    if len(sys.argv) != 3:
        print("Usage: make_fat32.py <output.img> <source_file>")
        sys.exit(1)
    
    create_fat32_image(sys.argv[1], sys.argv[2])
