#!/usr/bin/env python3
"""
HuseynOS Build System 
=====================
Automates: compile kernel -> prepare Limine -> create bootable ISO

Usage:
    python build.py              # Build ISO
    python build.py run          # Build ISO and launch in QEMU
    python build.py clean        # Clean build artifacts

Works on Windows without xorriso — uses pycdlib (pure Python) for ISO creation.
"""

import os
import sys
import shutil
import subprocess
import struct
from pathlib import Path

try:
    import pycdlib
except ImportError:
    print("ERROR: pycdlib not installed. Run: python -m pip install pycdlib")
    sys.exit(1)

# === Configuration ===
PROJECT_ROOT = Path(__file__).parent.resolve()
KERNEL_DIR = PROJECT_ROOT / "kernel"
BUILD_DIR = PROJECT_ROOT / "build"
ISO_ROOT = BUILD_DIR / "iso_root"
LIMINE_DIR = BUILD_DIR / "limine"
OUTPUT_ISO = BUILD_DIR / "huseynos.iso"

CARGO_TARGET = "x86_64-unknown-none"
KERNEL_BINARY = PROJECT_ROOT / "target" / CARGO_TARGET / "release" / "huseynos-kernel"

# Limine v8.x release
LIMINE_REPO = "https://github.com/limine-bootloader/limine"
LIMINE_BRANCH = "v8.x-binary"


def run_cmd(cmd: list[str], cwd: Path = PROJECT_ROOT, check: bool = True, env: dict | None = None) -> subprocess.CompletedProcess:
    """Run a command and stream output."""
    print(f"  > {' '.join(str(c) for c in cmd)}")
    result = subprocess.run(cmd, cwd=cwd, check=check, env=env)
    return result


def find_tool(name: str, extra_paths: list[str] | None = None) -> str | None:
    """Find an executable in PATH or common install locations."""
    found = shutil.which(name)
    if found:
        return found
    
    # Check common locations
    search_paths = extra_paths or []
    search_paths += [
        str(Path.home() / ".cargo" / "bin"),
        r"C:\Program Files\Git\bin",
        r"C:\Program Files\QEMU",
    ]
    
    for p in search_paths:
        candidate = Path(p) / (name if not sys.platform == "win32" else f"{name}.exe")
        if candidate.exists():
            return str(candidate)
    
    return None


# ─── Step 1: Build Kernel ────────────────────────────────────────────────────

def step_build_kernel():
    """Compile the kernel with Cargo."""
    print("\n" + "-" * 50)
    print("  [1/4] Building kernel and userland...")
    print("-" * 50)
    
    # We delegate to the PowerShell script because it sets up the correct RUSTFLAGS
    # for each userland crate to avoid linker errors.
    print("  Delegating build to run_tests.ps1...")
    run_cmd(["powershell", "-ExecutionPolicy", "Bypass", "-File", "run_tests.ps1", "-SkipISO"], cwd=PROJECT_ROOT)
    
    if not KERNEL_BINARY.exists():
        print(f"  ERROR: Kernel binary not found at {KERNEL_BINARY}")
        sys.exit(1)
    
    size_kb = KERNEL_BINARY.stat().st_size / 1024
    print(f"  OK: Kernel built: {KERNEL_BINARY.name} ({size_kb:.1f} KB)")


# ─── Step 2: Get Limine ──────────────────────────────────────────────────────

def step_get_limine():
    """Download Limine bootloader if not already present."""
    print("\n" + "-" * 50)
    print("  [2/4] Preparing Limine bootloader...")
    print("-" * 50)
    
    if (LIMINE_DIR / "BOOTX64.EFI").exists():
        print("  OK: Limine already available, skipping download.")
        return

    BUILD_DIR.mkdir(parents=True, exist_ok=True)
    
    if not LIMINE_DIR.exists():
        git = find_tool("git")
        if git:
            print("  Cloning Limine binary branch...")
            run_cmd([git, "clone", "--depth=1", "--branch", LIMINE_BRANCH,
                     LIMINE_REPO, str(LIMINE_DIR)])
        else:
            print("  ERROR: git not found! Install git and retry.")
            sys.exit(1)
    
    print("  OK: Limine bootloader ready.")


# ─── Step 3: Create ISO with pycdlib ─────────────────────────────────────────

def step_create_iso():
    """Create bootable ISO using pycdlib (pure Python, no xorriso needed)."""
    print("\n" + "-" * 50)
    print("  [3/4] Creating bootable ISO (pycdlib)...")
    print("-" * 50)
    
    iso = pycdlib.PyCdlib()
    iso.new(
        interchange_level=4,
        joliet=3,
        rock_ridge="1.09",
        vol_ident="HUSEYNOS",
    )
    
    # interchange_level=4 allows long filenames in ISO9660 paths
    
    import io
    
    iso.add_directory("/boot", joliet_path="/boot", rr_name="boot")
    iso.add_directory("/EFI", joliet_path="/EFI", rr_name="EFI")
    iso.add_directory("/EFI/BOOT", joliet_path="/EFI/BOOT", rr_name="BOOT")
    
    def add_file(src_path: Path, iso_path: str, joliet_path: str, rr_name: str):
        data = src_path.read_bytes()
        iso.add_fp(
            fp=io.BytesIO(data),
            length=len(data),
            iso_path=iso_path,
            joliet_path=joliet_path,
            rr_name=rr_name,
        )
        print(f"    + {rr_name} ({len(data) / 1024:.1f} KB)")
    
    # -- Add kernel binary --
    print("  Adding kernel...")
    add_file(KERNEL_BINARY, "/boot/kernel", "/boot/kernel", "kernel")
    
    # -- Add fs.img --
    fs_img_path = BUILD_DIR / "fs.img"
    if fs_img_path.exists():
        print("  Adding fs.img...")
        add_file(fs_img_path, "/fs.img", "/fs.img", "fs.img")
    else:
        print("  WARNING: fs.img not found, skipping.")

    # -- Add limine.conf to root AND /boot --
    print("  Adding limine.conf...")
    limine_conf = PROJECT_ROOT / "limine.conf"
    add_file(limine_conf, "/limine.conf", "/limine.conf", "limine.conf")
    data = limine_conf.read_bytes()
    iso.add_fp(
        fp=io.BytesIO(data), length=len(data),
        iso_path="/boot/limine.conf",
        joliet_path="/boot/limine.conf",
        rr_name="limine.conf",
    )
    
    # -- Add Limine bootloader files --
    print("  Adding Limine bootloader files...")
    limine_boot_files = {
        "limine-bios.sys":    ("/boot/limine-bios.sys",    "/boot/limine-bios.sys",    "limine-bios.sys"),
        "limine-bios-cd.bin": ("/boot/limine-bios-cd.bin", "/boot/limine-bios-cd.bin", "limine-bios-cd.bin"),
        "limine-uefi-cd.bin": ("/boot/limine-uefi-cd.bin", "/boot/limine-uefi-cd.bin", "limine-uefi-cd.bin"),
    }
    
    for filename, (iso_p, joliet_p, rr_n) in limine_boot_files.items():
        src = LIMINE_DIR / filename
        if src.exists():
            add_file(src, iso_p, joliet_p, rr_n)
    
    # -- Add UEFI boot files --
    efi_files = {
        "BOOTX64.EFI":  ("/EFI/BOOT/BOOTX64.EFI",  "/EFI/BOOT/BOOTX64.EFI",  "BOOTX64.EFI"),
        "BOOTIA32.EFI": ("/EFI/BOOT/BOOTIA32.EFI", "/EFI/BOOT/BOOTIA32.EFI", "BOOTIA32.EFI"),
    }
    
    for filename, (iso_p, joliet_p, rr_n) in efi_files.items():
        src = LIMINE_DIR / filename
        if src.exists():
            add_file(src, iso_p, joliet_p, rr_n)
    
    # -- El Torito boot records --
    bios_cd_bin = LIMINE_DIR / "limine-bios-cd.bin"
    uefi_cd_bin = LIMINE_DIR / "limine-uefi-cd.bin"
    
    if bios_cd_bin.exists():
        print("  Setting up El Torito BIOS boot record...")
        iso.add_eltorito(
            bootfile_path="/boot/limine-bios-cd.bin",
            bootcatfile="/boot/boot.cat",
            boot_load_size=4,
            media_name="noemul",
            boot_info_table=True,
        )
    
    if uefi_cd_bin.exists():
        print("  Adding UEFI El Torito boot entry...")
        iso.add_eltorito(
            bootfile_path="/boot/limine-uefi-cd.bin",
            efi=True,
            media_name="noemul",
        )
    
    # ── Write ISO ──
    print(f"\n  Writing ISO to: {OUTPUT_ISO}")
    OUTPUT_ISO.parent.mkdir(parents=True, exist_ok=True)
    iso.write(str(OUTPUT_ISO))
    iso.close()
    
    size_mb = OUTPUT_ISO.stat().st_size / (1024 * 1024)
    print(f"  OK: ISO created: {OUTPUT_ISO.name} ({size_mb:.1f} MB)")
    
    # ── Run limine bios-install (Windows .exe available) ──
    limine_exe = LIMINE_DIR / "limine.exe"
    if not limine_exe.exists():
        limine_exe = LIMINE_DIR / "limine"
    
    if limine_exe.exists():
        print("  Installing Limine BIOS boot sector...")
        run_cmd([str(limine_exe), "bios-install", str(OUTPUT_ISO)])
        print("  OK: BIOS boot sector installed.")
    else:
        print("  WARNING: limine executable not found — BIOS boot may not work.")
        print("    UEFI boot should still work in VirtualBox (enable EFI in VM settings).")


# ─── Step 4: Run in QEMU ─────────────────────────────────────────────────────

def step_run_qemu():
    print("\n" + "-" * 50)
    print("  [4/4] Launching in QEMU...")
    print("-" * 50)
    
    qemu = find_tool("qemu-system-x86_64")
    if not qemu:
        print("  QEMU not found. Skipping auto-launch.")
        print(f"  → Open VirtualBox, create a VM, and attach: {OUTPUT_ISO}")
        return
    
    run_cmd([
        qemu, "-cdrom", str(OUTPUT_ISO),
        "-serial", "file:qemu_out.txt",
        "-m", "128M",
        "-no-reboot",
        "-no-shutdown",
    ])


# ─── Clean ────────────────────────────────────────────────────────────────────

def clean():
    """Remove all build artifacts."""
    print("Cleaning build artifacts...")
    for d in [BUILD_DIR / "iso_root", PROJECT_ROOT / "target"]:
        if d.exists():
            shutil.rmtree(d)
            print(f"  Removed {d}")
    if OUTPUT_ISO.exists():
        OUTPUT_ISO.unlink()
        print(f"  Removed {OUTPUT_ISO}")
    print("✓ Done.")


# ─── Main ─────────────────────────────────────────────────────────────────────

def main():
    print("=" * 50)
    print("  HuseynOS Build System v0.1")
    print("=" * 50)
    
    if len(sys.argv) > 1:
        cmd = sys.argv[1].lower()
        if cmd == "clean":
            clean()
            return
        elif cmd == "run":
            step_build_kernel()
            step_get_limine()
            step_create_iso()
            step_run_qemu()
            return
        elif cmd == "run-qemu":
            step_run_qemu()
            return
        else:
            print(f"Unknown command: {cmd}")
            print("Usage: python build.py [run|run-qemu|clean]")
            sys.exit(1)
    
    # Default: build ISO
    # step_build_kernel()  # Disabled to use native PowerShell compilation instead
    step_get_limine()
    step_create_iso()
    
    print("\n" + "=" * 50)
    print("  OK: Build complete!")
    print(f"  ISO: {OUTPUT_ISO}")
    print("  Run in QEMU:      python build.py run")
    print("  Run in VirtualBox: Attach ISO as CD/DVD")
    print("=" * 50)


if __name__ == "__main__":
    main()
