#![no_std]
#![no_main]

use libhuseyn::{exit, getpid, yield_now};

#[unsafe(no_mangle)]
pub extern "C" fn _start() -> ! {
    let my_pid = getpid();
    
    for i in 0..5 {
        libhuseyn::println!("[hello.elf] Hello from fully isolated Ring 3 Userspace! (PID {}) Loop {}", my_pid, i);
        
        // Yield to let the Console driver process the pipeline
        for _ in 0..10 {
            yield_now();
        }
    }
    
    libhuseyn::println!("[hello.elf] Finished execution. Gracefully exiting...");
    exit(0);
}
