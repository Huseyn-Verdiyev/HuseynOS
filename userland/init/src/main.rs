#![no_std]
#![no_main]

use libhuseyn::{ipc, print, spawn, syscall, SYS_YIELD};

#[no_mangle]
pub extern "C" fn _start() -> ! {
    // 1. Spawn Console Driver (PID 2)
    let _ = spawn("console.elf");

    // Yield briefly so console driver maps framebuffer before receiving logs
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }
    unsafe { syscall(SYS_YIELD, 0, 0, 0, 0, 0, 0); }

    print!("[init] console.elf started (PID 2).\n");

    // 2. Spawn Keyboard Driver (PID 3)
    print!("[init] Spawning keyboard.elf (PID 3)...\n");
    match spawn("keyboard.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn keyboard.elf\n"),
    }

    // 3. Spawn Mouse Driver (PID 4)
    print!("[init] Spawning mouse.elf (PID 4)...\n");
    match spawn("mouse.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn mouse.elf\n"),
    }

    // 4. Spawn Monolith Compositor (PID 5)
    print!("[init] Spawning comp.elf (PID 5)...\n");
    match spawn("comp.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn comp.elf\n"),
    }

    // 5. Spawn Monolith GUI Terminal (PID 6)
    print!("[init] Spawning term.elf (PID 6)...\n");
    match spawn("term.elf") {
        Ok(_) => print!("[init]  -> Success\n"),
        Err(_) => print!("[init]  -> Failed to spawn term.elf\n"),
    }

    print!("[init] All userland services online. Entering low-power sleep.\n");

    // Block on IPC receive instead of busy-yielding so PID 1 consumes zero CPU cycles
    loop {
        let _ = ipc::receive();
    }
}
