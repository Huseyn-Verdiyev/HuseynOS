//! Bare-metal ATA/PIO 28-bit LBA Hardware Block Driver.
//! Enables HuseynOS to identify, read, and write sectors on physical/virtual ATA/IDE/SATA-legacy disks.

use core::arch::asm;
use spin::Mutex;
use crate::println;

const ATA_PRIMARY_DATA: u16       = 0x1F0;
const ATA_PRIMARY_SECCOUNT: u16   = 0x1F2;
const ATA_PRIMARY_LBA_LO: u16     = 0x1F3;
const ATA_PRIMARY_LBA_MID: u16    = 0x1F4;
const ATA_PRIMARY_LBA_HI: u16     = 0x1F5;
const ATA_PRIMARY_DRIVE_HEAD: u16 = 0x1F6;
const ATA_PRIMARY_STATUS_CMD: u16 = 0x1F7;
const ATA_PRIMARY_CTRL: u16       = 0x3F6;

const ATA_CMD_READ_PIO: u8        = 0x20;
const ATA_CMD_WRITE_PIO: u8       = 0x30;
const ATA_CMD_CACHE_FLUSH: u8     = 0xE7;
const ATA_CMD_IDENTIFY: u8        = 0xEC;

const ATA_SR_BSY: u8              = 0x80;
const ATA_SR_DRQ: u8              = 0x08;
const ATA_SR_ERR: u8              = 0x01;

static ATA_PRESENT: Mutex<bool> = Mutex::new(false);

#[inline]
unsafe fn outb(port: u16, val: u8) {
    asm!("out dx, al", in("dx") port, in("al") val, options(nomem, nostack, preserves_flags));
}

#[inline]
unsafe fn inb(port: u16) -> u8 {
    let val: u8;
    asm!("in al, dx", out("al") val, in("dx") port, options(nomem, nostack, preserves_flags));
    val
}

#[inline]
unsafe fn inw(port: u16) -> u16 {
    let val: u16;
    asm!("in ax, dx", out("ax") val, in("dx") port, options(nomem, nostack, preserves_flags));
    val
}

#[inline]
unsafe fn outw(port: u16, val: u16) {
    asm!("out dx, ax", in("dx") port, in("ax") val, options(nomem, nostack, preserves_flags));
}

unsafe fn io_delay() {
    for _ in 0..4 {
        let _ = inb(ATA_PRIMARY_CTRL);
    }
}

unsafe fn wait_not_busy() -> bool {
    for _ in 0..100_000 {
        let status = inb(ATA_PRIMARY_STATUS_CMD);
        if status == 0xFF {
            return false; // Floating bus
        }
        if (status & ATA_SR_BSY) == 0 {
            return true;
        }
    }
    false
}

unsafe fn wait_drq() -> bool {
    for _ in 0..100_000 {
        let status = inb(ATA_PRIMARY_STATUS_CMD);
        if (status & ATA_SR_ERR) != 0 {
            return false;
        }
        if (status & ATA_SR_BSY) == 0 && (status & ATA_SR_DRQ) != 0 {
            return true;
        }
    }
    false
}

/// Probe Primary Master ATA disk using IDENTIFY.
pub fn init() {
    unsafe {
        let status = inb(ATA_PRIMARY_STATUS_CMD);
        if status == 0xFF {
            println!("[ATA] Floating bus on Primary channel (no physical IDE disk attached)");
            return;
        }

        outb(ATA_PRIMARY_DRIVE_HEAD, 0xA0); // Select Master
        io_delay();
        outb(ATA_PRIMARY_SECCOUNT, 0);
        outb(ATA_PRIMARY_LBA_LO, 0);
        outb(ATA_PRIMARY_LBA_MID, 0);
        outb(ATA_PRIMARY_LBA_HI, 0);
        outb(ATA_PRIMARY_STATUS_CMD, ATA_CMD_IDENTIFY);
        io_delay();

        let st = inb(ATA_PRIMARY_STATUS_CMD);
        if st == 0 || !wait_not_busy() {
            println!("[ATA] Primary Master not present or ATAPI device");
            return;
        }

        // Check if LBA Mid/Hi are non-zero (indicates ATAPI/SATA packet device, not standard ATA disk)
        let lba_mid = inb(ATA_PRIMARY_LBA_MID);
        let lba_hi = inb(ATA_PRIMARY_LBA_HI);
        if lba_mid != 0 || lba_hi != 0 {
            println!("[ATA] Primary Master is ATAPI optical drive (LBA sig {:02X}:{:02X})", lba_mid, lba_hi);
            return;
        }

        if !wait_drq() {
            return;
        }

        let mut ident = [0u16; 256];
        for word in ident.iter_mut() {
            *word = inw(ATA_PRIMARY_DATA);
        }

        let sectors_28 = (ident[60] as u32) | ((ident[61] as u32) << 16);
        let size_mb = sectors_28 / 2048;
        *ATA_PRESENT.lock() = sectors_28 > 0;
        println!("[OK] ATA Primary Master online ({} sectors, ~{} MiB)", sectors_28, size_mb);
    }
}

pub fn is_available() -> bool {
    *ATA_PRESENT.lock()
}

/// Read a single 512-byte sector using 28-bit LBA PIO.
pub fn read_sector(lba: u32, buf: &mut [u8; 512]) -> bool {
    if !is_available() {
        return false;
    }
    unsafe {
        if !wait_not_busy() {
            return false;
        }
        outb(ATA_PRIMARY_DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
        outb(ATA_PRIMARY_SECCOUNT, 1);
        outb(ATA_PRIMARY_LBA_LO, (lba & 0xFF) as u8);
        outb(ATA_PRIMARY_LBA_MID, ((lba >> 8) & 0xFF) as u8);
        outb(ATA_PRIMARY_LBA_HI, ((lba >> 16) & 0xFF) as u8);
        outb(ATA_PRIMARY_STATUS_CMD, ATA_CMD_READ_PIO);

        if !wait_drq() {
            return false;
        }

        for i in 0..256 {
            let w = inw(ATA_PRIMARY_DATA);
            buf[i * 2] = (w & 0xFF) as u8;
            buf[i * 2 + 1] = (w >> 8) as u8;
        }
        true
    }
}

/// Write a single 512-byte sector using 28-bit LBA PIO.
pub fn write_sector(lba: u32, buf: &[u8; 512]) -> bool {
    if !is_available() {
        return false;
    }
    unsafe {
        if !wait_not_busy() {
            return false;
        }
        outb(ATA_PRIMARY_DRIVE_HEAD, 0xE0 | (((lba >> 24) & 0x0F) as u8));
        outb(ATA_PRIMARY_SECCOUNT, 1);
        outb(ATA_PRIMARY_LBA_LO, (lba & 0xFF) as u8);
        outb(ATA_PRIMARY_LBA_MID, ((lba >> 8) & 0xFF) as u8);
        outb(ATA_PRIMARY_LBA_HI, ((lba >> 16) & 0xFF) as u8);
        outb(ATA_PRIMARY_STATUS_CMD, ATA_CMD_WRITE_PIO);

        if !wait_drq() {
            return false;
        }

        for i in 0..256 {
            let w = (buf[i * 2] as u16) | ((buf[i * 2 + 1] as u16) << 8);
            outw(ATA_PRIMARY_DATA, w);
        }

        outb(ATA_PRIMARY_STATUS_CMD, ATA_CMD_CACHE_FLUSH);
        wait_not_busy()
    }
}
