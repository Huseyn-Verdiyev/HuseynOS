#![no_std]
#![no_main]

use libhuseyn::{print, spawn, syscall, SYS_YIELD};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // Spawn Console Driver (we can't print anything yet!)
    let _ = spawn("console.elf");

    // Yield to let console initialize BEFORE keyboard starts sending it data
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }

    // Now it's safe to print!
    print!("[init] console.elf started.\n");

    // Spawn Keyboard Driver
    print!("[init] Spawning keyboard.elf...\n");
    match spawn("keyboard.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn keyboard.elf\n"),
    }

    // Spawn Mouse Driver (PID 5)
    print!("[init] Spawning mouse.elf...\n");
    match spawn("mouse.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn mouse.elf\n"),
    }

    // Spawn Compositor (PID 6)
    print!("[init] Spawning comp.elf...\n");
    match spawn("comp.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn comp.elf\n"),
    }

    // Spawn GUI Terminal (PID 7)
    print!("[init] Spawning term.elf...\n");
    match spawn("term.elf") {
        Ok(_) => print!("[init]  -> Success (PID: 7)\n"),
        Err(_) => print!("[init]  -> Failed to spawn term.elf\n"),
    }

    print!("[init] All services started. Init process going to sleep.\n");

    // Init just hangs around as PID 1
    loop {
        unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }
    }
}
