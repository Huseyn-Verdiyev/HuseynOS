use spin::Mutex;
use core::arch::asm;

const CMOS_ADDRESS: u16 = 0x70;
const CMOS_DATA: u16 = 0x71;

/// The current Date and Time read from the RTC
#[derive(Debug, Clone, Copy)]
pub struct DateTime {
    pub second: u8,
    pub minute: u8,
    pub hour: u8,
    pub day: u8,
    pub month: u8,
    pub year: u16,
}

static RTC_MUTEX: Mutex<()> = Mutex::new(());

/// Read a value from a specific CMOS register
fn read_register(reg: u8) -> u8 {
    let mut data: u8;
    unsafe {
        asm!("out dx, al", in("dx") CMOS_ADDRESS, in("al") reg, options(preserves_flags, nomem, nostack));
        asm!("in al, dx", out("al") data, in("dx") CMOS_DATA, options(preserves_flags, nomem, nostack));
    }
    data
}

/// Check if the RTC is currently updating (to avoid reading volatile state).
fn is_updating() -> bool {
    let mut data: u8;
    unsafe {
        asm!("out dx, al", in("dx") CMOS_ADDRESS, in("al") 0x0A_u8, options(preserves_flags, nomem, nostack));
        asm!("in al, dx", out("al") data, in("dx") CMOS_DATA, options(preserves_flags, nomem, nostack));
    }
    (data & 0x80) != 0
}

/// Convert BCD (Binary Coded Decimal) to binary.
fn bcd_to_binary(bcd: u8) -> u8 {
    (bcd & 0x0F) + ((bcd / 16) * 10)
}

/// Read the current RTC time.
pub fn read_datetime() -> DateTime {
    // Lock to prevent races if multiple tasks try to read RTC simultaneously.
    let _lock = RTC_MUTEX.lock();

    // Wait until RTC is not updating
    while is_updating() {}

    let mut second = read_register(0x00);
    let mut minute = read_register(0x02);
    let mut hour = read_register(0x04);
    let mut day = read_register(0x07);
    let mut month = read_register(0x08);
    let mut year = read_register(0x09) as u16;

    let register_b = read_register(0x0B);

    // If BCD format is used (bit 2 is 0), convert to binary.
    if (register_b & 0x04) == 0 {
        second = bcd_to_binary(second);
        minute = bcd_to_binary(minute);
        // Handle 12-hour format if applicable
        let pm = (hour & 0x80) != 0;
        hour = bcd_to_binary(hour & 0x7F);
        if pm {
            // Need to handle PM correctly based on 12-hour flag (bit 1 of Reg B).
            // Usually, hour is just BCD converted.
            hour = (hour + 12) % 24;
        }
        
        day = bcd_to_binary(day);
        month = bcd_to_binary(month);
        year = bcd_to_binary(year as u8) as u16;
    }

    // Convert 12-hour clock to 24-hour clock if bit 1 is 0
    if (register_b & 0x02) == 0 && (hour & 0x80) != 0 {
        hour = ((hour & 0x7F) + 12) % 24;
    }

    // Assuming year is 20th/21st century
    let century = read_register(0x32);
    if century != 0 {
        let century = if (register_b & 0x04) == 0 {
            bcd_to_binary(century) as u16
        } else {
            century as u16
        };
        year += century * 100;
    } else {
        // Fallback if century register isn't present
        year += if year < 80 { 2000 } else { 1900 };
    }

    DateTime {
        second,
        minute,
        hour,
        day,
        month,
        year,
    }
}
