# HuseynOS
🦀 A 64-bit microkernel OS built from scratch in Rust by Huseyn Verdiyev. Features preemptive multitasking, Limine bootloader, and a custom FAT12 ramdisk.

HuseynOS is a 64-bit operating system built from scratch in **Rust** by Huseyn Verdiyev 🦀.
It features a custom bootloader setup (Limine), a microkernel architecture, preemptive multitasking, and a custom FAT12 ramdisk filesystem. 

This project is an educational journey into operating system development, demonstrating modern bare-metal programming techniques using Rust's safety guarantees and zero-cost abstractions.

---

## ✨ Features (v0.5.0)

Current capabilities of HuseynOS include:

- 🛡️ **64-bit Architecture**: Fully operates in x86_64 Long Mode.
- 🥾 **Limine Bootloader**: Modern, reliable booting adhering to the Limine Boot Protocol with Higher Half Direct Mapping (HHDM).
- 🧠 **Memory Management**: 
  - Physical Page Frame Allocator (Bitmap based).
  - Virtual Memory Paging (4-level page tables).
  - Kernel Heap Allocator (`linked_list_allocator`).
- ⏱️ **Interrupts & Hardware**:
  - Global Descriptor Table (GDT) & Task State Segment (TSS).
  - Interrupt Descriptor Table (IDT).
  - PIC Remapping & Exception Handling (Double Faults, Page Faults, etc).
  - Programmable Interval Timer (PIT) configured at 1000 Hz.
  - Real-Time Clock (RTC) reading from CMOS.
- 🔄 **Preemptive Multitasking**:
  - Timer-driven (IRQ0) context switching.
  - Process structures with dedicated kernel stacks.
  - Safe state saving/restoring via hardware interrupt frames.
  - System Calls (`int 0x80`) including `yield`.
- ⌨️ **Input & Output**:
  - PS/2 Keyboard driver with scancode translation, Shift, and CapsLock support.
  - VGA Framebuffer console output (using Limine's provided framebuffer) with custom 8x16 font rendering and scrolling.
  - Serial port (COM1) debugging output.
- 📁 **Filesystem (FAT12)**:
  - Custom FAT12 parser reading directly from a Limine Boot Module mapped in RAM.
  - Support for `ls` (directory listing) and `cat` (file reading).
- 💻 **Interactive Shell**:
  - Built-in `root@huseynos:~$` CLI.
  - Available commands: `help`, `info`, `clear`, `date`, `ls`, `cat`.

---

## 🏗️ Project Structure

```text
HuseynOS/
├── build.py           # Python build & automation script
├── limine.conf        # Limine bootloader configuration
├── make_fat.py        # Pure-Python script to generate the FAT12 fs.img
├── fsroot/            # Contents injected into the FAT12 filesystem image
│   └── hello.txt      # Example text file
├── kernel/            # The Rust microkernel source code
│   ├── Cargo.toml
│   ├── linker.ld      # Linker script for memory layout
│   └── src/
│       ├── main.rs    # Kernel entry point
│       ├── console.rs # Framebuffer graphics & text rendering
│       ├── idt.rs     # Interrupts & Exception handling
│       ├── process.rs # Process management & context structs
│       ├── scheduler.rs # Preemptive task scheduling 
│       ├── fat32.rs   # FAT12/16/32 filesystem parser
│       ├── shell.rs   # Interactive command line interface
│       ├── keyboard.rs# PS/2 scancode translator
│       ├── serial.rs  # COM1 logging
│       ├── pit.rs     # Programmable Interval Timer (1000Hz)
│       └── rtc.rs     # CMOS Date & Time
```

---

## 🛠️ Build Instructions

### Prerequisites

You need the following installed on your system (tested on Windows):
1. **[Rust](https://rustup.rs/)** (Nightly channel)
2. **[Python 3](https://www.python.org/downloads/)**
3. **[QEMU](https://www.qemu.org/download/)** (Added to your system `PATH`)
4. **Git** bash/tools (optional but recommended)

Set your default Rust toolchain to nightly and install the required components:
```bash
rustup default nightly
rustup component add rust-src
rustup target add x86_64-unknown-none
```

### Compiling and Running

Everything is automated via the `build.py` script. 

To build the kernel, generate the FAT12 filesystem image, bundle them into a bootable ISO using Limine, and immediately launch QEMU:

```bash
python build.py run
```

*Note: The script automatically handles downloading the Limine bootloader binaries during the first run.*

To just build the ISO without running it:
```bash
python build.py iso
```

To clean the build artifacts:
```bash
python build.py clean
```

---

## 🎮 Interacting with the OS

Once QEMU boots up HuseynOS, you will be greeted by the interactive shell.

Click inside the QEMU window to capture your keyboard, and try the following commands:
- `help` : View available commands.
- `info` : View OS architecture and feature status.
- `date` : Fetch the current hardware time using the RTC.
- `ls` : List the files present on the injected FAT12 ramdisk.
- `cat hello.txt` : Read the contents of a file on the disk.
- *(Type really fast while running a command to test the preemptive multitasking!)*

To release your mouse/keyboard from QEMU, press `Ctrl + Alt`.

---

## 🗺️ Roadmap / Next Phases

- [x] Phase 1: Bootloader & Screen (Limine + Framebuffer)
- [x] Phase 2: Memory (GDT, IDT, Paging, Heap)
- [x] Phase 3: Input & Shell (PS/2 Keyboard, CLI)
- [x] Phase 4: Cooperative Multitasking (Processes & Syscalls)
- [x] Phase 5a: True Preemptive Multitasking & RTC & FAT12
- [ ] Phase 5b: ELF Binary Loader & Userland (Ring 3) Transition
- [ ] Phase 6: IPC (Inter-Process Communication) and Microkernel Driver separation

---
*Built with ❤️ in Rust.*
