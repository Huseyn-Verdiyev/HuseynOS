/// VFS — Virtual File System layer with per-process file descriptor tables.
///
/// Provides SYS_OPEN / SYS_READ / SYS_WRITE / SYS_CLOSE / SYS_STAT / SYS_LIST_DIR
/// operating on the existing in-memory FAT12 ramdisk.

use alloc::string::String;
use alloc::vec::Vec;

/// Maximum open files per process.
pub const MAX_FDS: usize = 16;

/// A file descriptor entry — tracks an open file.
#[derive(Clone)]
pub struct FileDescriptor {
    pub filename: String,
    pub data: Vec<u8>,
    pub offset: usize,  // current read/write position
    pub writable: bool,
}

/// Per-process file descriptor table.
#[derive(Clone)]
pub struct FdTable {
    pub fds: [Option<FileDescriptor>; MAX_FDS],
}

impl FdTable {
    pub const fn new() -> Self {
        // Can't use const None with generic Option<FileDescriptor>, so we init at runtime
        Self {
            fds: [
                None, None, None, None,
                None, None, None, None,
                None, None, None, None,
                None, None, None, None,
            ],
        }
    }

    /// Allocate the next free FD slot. Returns the fd number or None.
    pub fn alloc_fd(&mut self) -> Option<usize> {
        // FDs 0,1,2 are reserved for stdin/stdout/stderr conceptually
        // but we start from 3 for file I/O
        for i in 3..MAX_FDS {
            if self.fds[i].is_none() {
                return Some(i);
            }
        }
        None
    }

    /// Open a file by reading it from FAT12 ramdisk.
    pub fn open(&mut self, filename: &str, flags: u64) -> Option<usize> {
        let fd = self.alloc_fd()?;
        let writable = flags & 1 != 0; // O_WRONLY or O_RDWR

        let data = if let Some(existing) = crate::fat32::read_file(filename) {
            existing
        } else if writable {
            // Create new empty file if opening for write and file doesn't exist
            Vec::new()
        } else {
            return None; // File not found and read-only
        };

        self.fds[fd] = Some(FileDescriptor {
            filename: String::from(filename),
            data,
            offset: 0,
            writable,
        });
        Some(fd)
    }

    /// Read up to `count` bytes from fd into a buffer. Returns bytes read.
    pub fn read(&mut self, fd: usize, buf: &mut [u8], count: usize) -> usize {
        if fd >= MAX_FDS {
            return 0;
        }
        if let Some(ref mut file) = self.fds[fd] {
            let remaining = file.data.len().saturating_sub(file.offset);
            let to_read = count.min(remaining);
            if to_read > 0 {
                buf[..to_read].copy_from_slice(&file.data[file.offset..file.offset + to_read]);
                file.offset += to_read;
            }
            to_read
        } else {
            0
        }
    }

    /// Write `count` bytes to fd. Returns bytes written.
    pub fn write(&mut self, fd: usize, buf: &[u8], count: usize) -> usize {
        if fd >= MAX_FDS {
            return 0;
        }
        if let Some(ref mut file) = self.fds[fd] {
            if !file.writable {
                return 0;
            }
            let to_write = count.min(buf.len());
            // Extend file if writing past end
            let needed = file.offset + to_write;
            if needed > file.data.len() {
                file.data.resize(needed, 0);
            }
            file.data[file.offset..file.offset + to_write].copy_from_slice(&buf[..to_write]);
            file.offset += to_write;
            to_write
        } else {
            0
        }
    }

    /// Close a file descriptor.
    pub fn close(&mut self, fd: usize) -> bool {
        if fd >= MAX_FDS {
            return false;
        }
        if self.fds[fd].is_some() {
            self.fds[fd] = None;
            true
        } else {
            false
        }
    }

    /// Get file size (stat). Returns file size or u64::MAX on error.
    pub fn stat(&self, fd: usize) -> u64 {
        if fd >= MAX_FDS {
            return u64::MAX;
        }
        if let Some(ref file) = self.fds[fd] {
            file.data.len() as u64
        } else {
            u64::MAX
        }
    }
}
