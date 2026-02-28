use std::fs::File;
use std::io::Write;
use fscommon::BufStream;
use std::env;
use std::path::Path;

fn main() -> std::io::Result<()> {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("Usage: fat32-builder <output.img> [<file_to_copy>...]");
        std::process::exit(1);
    }

    let img_path = &args[1];

    // Create a 32MB file filled with zeros
    let mut file = File::create(img_path)?;
    let size = 32 * 1024 * 1024;
    file.set_len(size)?;
    file.sync_all()?;

    // Open it for fatfs
    let img_file = std::fs::OpenOptions::new().read(true).write(true).open(img_path)?;
    let buf_stream = BufStream::new(img_file);

    // Format as FAT32
    fatfs::format_volume(&buf_stream, fatfs::FormatVolumeOptions::new().fat_type(fatfs::FatType::Fat32))?;
    
    // Open the filesystem
    let fs = fatfs::FileSystem::new(buf_stream, fatfs::FsOptions::new())?;
    let root_dir = fs.root_dir();

    // Copy files
    for arg in args.iter().skip(2) {
        let path = Path::new(arg);
        if let Some(file_name) = path.file_name() {
            let name_str = file_name.to_string_lossy().into_owned();
            let mut new_file = root_dir.create_file(&name_str)?;
            let content = std::fs::read(path)?;
            new_file.write_all(&content)?;
            println!("Copied {} to FAT32 image.", name_str);
        }
    }

    println!("FAT32 image created successfully at {}", img_path);
    Ok(())
}
