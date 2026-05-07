---

## 1. 🌟 The Hook

<div align="center">
  <h1>🛡️ HuseynOS</h1>
  <p><strong>A modern, formally-inspired 64-bit operating system built entirely from scratch in Rust, leveraging a pure microkernel architecture for maximum safety and efficiency.</strong></p>
</div>

**About the Author:** This operating system was developed completely from scratch by **Huseyn Verdiyev**, a 15-year-old high school student from Azerbaijan with a deep passion for low-level systems engineering and memory-safe architectures.

**Description:** HuseynOS is an educational operating system built to tackle the inherent security and stability issues of monolithic kernels by strictly isolating device drivers and GUI services into unprivileged Userland processes. It demonstrates how modern memory-safe languages like Rust can be utilized at the bare-metal level to create highly resilient, fault-tolerant architectures without sacrificing performance.

---

## 2. 🏗️ Technical Architecture (Engineering Approach)

This project was developed **100% from scratch without any external OS templates or standard libraries (`#![no_std]`)**. Every core component, from the Global Descriptor Table to the physical page allocator, was architected from the ground up.

* **Pure Microkernel Design:** Unlike monolithic kernels (Linux, Windows) where drivers run with full kernel privileges, HuseynOS isolates almost everything. The kernel only handles memory paging, task scheduling, and IPC. Device drivers (Keyboard, Mouse) and the Graphical Compositor run as unprivileged Ring 3 Userland processes. If a driver crashes, the system survives.
* **Rust & Memory Safety:** By leveraging Rust's ownership and lifetime models at the bare-metal level, HuseynOS inherently prevents entire classes of kernel panics, such as segmentation faults, buffer overflows, and use-after-free bugs, without sacrificing zero-cost abstractions.
* **Compositor & GUI Rendering:** The GUI avoids tearing via a custom Server-Side Compositor. The compositor process directly maps the physical framebuffer into its virtual address space. It pre-computes complex gradients and static assets into a background cache buffer, performing rapid `memcpy` operations and Z-order sorting before blitting to the screen. All UI events are routed via a strict, synchronous message-passing IPC system (`int 0x80`).

---

## 2. ✨ Key Features (Phase 9)

* **Preemptive Multitasking:** True hardware-driven context switching utilizing the Programmable Interval Timer (PIT) bound to IRQ0, forcefully interrupting and scheduling Ring 3 processes.
* **Advanced Window Management:** Server-side window decoration with draggable title bars, dynamic Z-order focus (bringing clicked windows to the front), and graceful process termination via `MSG_QUIT` IPC signals.
* **RTC Integration & ACPI:** Real-Time Clock reading directly from CMOS integrated into a dynamic taskbar, alongside programmatic system shutdown via ACPI port triggers.
* **Dynamic Memory Management:** 4-level paging architecture (`PML4 -> PDPT -> PD -> PT`), demand paging, and a custom Bitmap-based Physical Page Frame Allocator.

---

## 3. 🧠 Technologies Used

* **Language:** Rust (Nightly toolchain, bare-metal `#![no_std]`)
* **Bootloader:** Limine Boot Protocol (Higher Half Direct Mapping)
* **Emulation & Testing:** QEMU
* **Executable Format:** Custom ELF64 binary loader
* **Filesystem:** FAT12 RAM-disk parser

---

## 4. 🛠️ Build & Run Instructions

To compile the microkernel, assemble the FAT12 filesystem image, and launch the OS in QEMU, follow these steps:

```bash
# 1. Install Rust Nightly and the required target
rustup default nightly
rustup component add rust-src
rustup target add x86_64-unknown-none

# 2. Build the OS and launch QEMU (using the automated Python build system)
python build.py run
```

---

## 5. 🗺️ Roadmap (Future Goals)

The foundation is rock-solid. The next phases of HuseynOS will focus on expanding capabilities:
* **Advanced Filesystems:** Transitioning from the FAT12 RAM-disk to full FAT32 and ext4 disk support.
* **Network Stack:** Implementing a custom TCP/IP stack and integrating VirtIO network drivers.
* **User-Mode Applications:** Expanding the standard library (`libhuseyn`) to support complex 3rd-party ports.
* **Symmetric Multiprocessing (SMP):** Waking up Application Processors (APs) for true multi-core execution.

---

## 6. 📸 Visual Proofs

![HuseynOS Desktop](screenshot.png)

*(Above: The HuseynOS Beta Desktop Environment showcasing the custom Compositor, draggable terminal windows, and RTC taskbar.)*

---
*Architected and engineered by Huseyn Verdiyev.*
