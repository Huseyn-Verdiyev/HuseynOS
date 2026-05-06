<div align="center">
  <h1>🛡️ HuseynOS</h1>
  <p><strong>A Modern, Formally-Inspired 64-bit Microkernel Operating System Written in Rust</strong></p>
  <p><i>Developed entirely from scratch to demonstrate advanced systems programming, bare-metal architecture, and the power of memory-safe abstractions.</i></p>
</div>

---

## 📌 Executive Summary

**HuseynOS** is an independent, educational operating system built from the ground up in **Rust**. Operating strictly in `x86_64` Long Mode, it eschews monolithic design in favor of a **pure microkernel architecture**. By pushing device drivers, filesystems, and the graphical compositor into fully isolated Userland (Ring 3) processes, HuseynOS achieves high fault tolerance and modularity, relying on a robust **Inter-Process Communication (IPC)** mechanism for system orchestration.

This project was developed not by assembling existing libraries, but by writing the core foundational layers—from the IDT and GDT to the physical page frame allocator and custom ELF loaders—completely from scratch.

## 🚀 Architectural Highlights

### 1. Pure Microkernel & IPC Design
Unlike Linux or Windows, the HuseynOS kernel does virtually no high-level processing. Its sole responsibilities are:
- **Memory Management** (Hardware paging, TLB, Page Frame allocation)
- **Task Scheduling** (Timer-driven preemptive context switching)
- **Inter-Process Communication (IPC)**

Everything else—the PS/2 Keyboard/Mouse drivers, the Console, the Graphical Compositor, and the Terminal—runs in isolated Ring 3 Userland processes. If the mouse driver crashes, the system survives. They communicate via a strict, synchronous message-passing IPC system using hardware interrupts (`int 0x80`).

### 2. Preemptive Multitasking & Ring 3 Userland
- **Hardware-driven Context Switching:** Utilizes the Programmable Interval Timer (PIT) bound to IRQ0 to forcefully interrupt processes, saving their CPU registers onto a dedicated kernel stack, and jumping to the next task in the queue.
- **Privilege Separation:** Processes are stripped of kernel privileges, operating in Ring 3 with localized Virtual Address Spaces. 
- **Custom ELF Loader:** The kernel parses Executable and Linkable Format (ELF) binaries dynamically, mapping their segments into virtual memory at runtime before execution.

### 3. Advanced Memory Management
- **Demand Paging:** Implements a 4-level page table architecture (`PML4 -> PDPT -> PD -> PT`).
- **Physical Memory:** A custom Bitmap-based Physical Page Frame Allocator manages available RAM retrieved from the bootloader's memory map.
- **Kernel Heap:** Implements a `linked_list_allocator` for dynamic data structures (Vectors, Strings) within the kernel's strictly typed boundary.

### 4. Custom Server-Side Graphical Compositor
HuseynOS features a fully functional Desktop Environment:
- **Server-Side Window Decoration:** The Compositor process handles all window borders, title bars, and close buttons. Applications just render their client area.
- **Zero-Tearing Double Buffering:** The screen is rendered to a background buffer cache (pre-calculating gradients and static assets via `memcpy`) before a single fast `blit` to the physical framebuffer.
- **Z-Order & Event Routing:** Dynamic window layering and coordinate-based event routing (mouse clicks, dragging) via the IPC subsystem.

## 🧠 Technical Stack & Methodologies

* **Language:** Rust (`#![no_std]`, `#![no_main]`)
* **Bootloader:** Limine Boot Protocol (Higher Half Direct Mapping)
* **Executable Format:** ELF64
* **Filesystem:** Custom FAT12 parser running over a RAM-disk implementation.
* **Build System:** A custom Python automation pipeline that compiles userland binaries, injects them into a FAT12 image, links the kernel, and packages a bootable `.iso`.

## 📂 Source Code Topology

```text
HuseynOS/
├── kernel/             # The Core Microkernel (Ring 0)
│   ├── src/memory/     # Paging, Frame Allocation, Heap
│   ├── src/process/    # Context Switching, Scheduler, ELF Loading
│   ├── src/ipc.rs      # Message-passing mechanisms
│   └── src/idt.rs      # Interrupt Descriptor Table & ISRs
├── userland/           # Isolated Processes (Ring 3)
│   ├── compositor/     # The GUI Window Manager
│   ├── terminal/       # CLI Interface & App
│   ├── init/           # PID 1 - System initialization and process spawning
│   ├── mouse_driver/   # PS/2 Mouse Controller
│   └── keyboard_driver/# PS/2 Keyboard Controller
├── libhuseyn/          # Standard Library alternative for HuseynOS applications
│   └── src/ipc.rs      # Syscall wrappers
└── build.py            # Automated build, image generation, and QEMU orchestration
```

## 🛠️ Build & Run Instructions

**Prerequisites:** Rust Nightly, Python 3, QEMU, Git.

```bash
# 1. Setup Rust toolchain
rustup default nightly
rustup component add rust-src
rustup target add x86_64-unknown-none

# 2. Build the OS, construct the FAT12 disk, generate ISO, and launch QEMU
python build.py run
```

## 🎓 Educational Value & Motivation

Building HuseynOS was an exercise in extreme low-level systems engineering. It required reading Intel Software Developer Manuals, understanding the legacy complexities of the x86 architecture, debugging Triple Faults with GDB attached to QEMU, and implementing complex algorithms (like color interpolation and Z-order sorting) without the safety net of an underlying operating system or standard library.

It stands as a testament to the viability of Rust in OS development, leveraging Ownership, Lifetimes, and safe abstractions to eliminate entire classes of memory bugs (buffer overflows, use-after-free) at the kernel level.

---
*Architected and engineered by Huseyn Verdiyev.*
