param(
    [switch]$SkipISO,
    [switch]$RunInQemu
)

$ErrorActionPreference = "Continue"

$env:PATH = "$env:USERPROFILE\.cargo\bin;$env:USERPROFILE\AppData\Local\Programs\Python\Python312;$env:USERPROFILE\AppData\Local\Programs\Python\Python312\Scripts;C:\Program Files\Git\bin;$env:PATH"

# Ensure we run from the OS project root
Set-Location $PSScriptRoot

Write-Host "Building Kernel..."
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Write-Error "Kernel build failed!"; exit 1 }

Write-Host "Building Userland (hello_user)..."
Push-Location "userland\hello_user"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "hello_user build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (console_driver)..."
Push-Location "userland\console_driver"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "console_driver build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (keyboard_driver)..."
Push-Location "userland\keyboard_driver"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "keyboard_driver build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (init)..."
Push-Location "userland\init"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "init build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (shell)..."
Push-Location "userland\shell"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "shell build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (mouse_driver)..."
Push-Location "userland\mouse_driver"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "mouse_driver build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (compositor)..."
Push-Location "userland\compositor"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "compositor build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Building Userland (terminal)..."
Push-Location "userland\terminal"
$env:RUSTFLAGS = "-C link-arg=-Tlinker.ld -C relocation-model=static -C panic=abort"
cargo build --release --target x86_64-unknown-none
if ($LASTEXITCODE -ne 0) { Pop-Location; Write-Error "terminal build failed!"; exit 1 }
$env:RUSTFLAGS = ""
Pop-Location

Write-Host "Copying Userland binaries..."
New-Item -ItemType Directory -Force "fsroot" | Out-Null
Copy-Item "userland\hello_user\target\x86_64-unknown-none\release\hello_user" "fsroot\hello.elf" -Force
Copy-Item "userland\console_driver\target\x86_64-unknown-none\release\console_driver" "fsroot\console.elf" -Force
Copy-Item "userland\keyboard_driver\target\x86_64-unknown-none\release\keyboard_driver" "fsroot\keyboard.elf" -Force
Copy-Item "target\x86_64-unknown-none\release\init" "fsroot\init.elf" -Force
Copy-Item "target\x86_64-unknown-none\release\shell" "fsroot\shell.elf" -Force
Copy-Item "userland\mouse_driver\target\x86_64-unknown-none\release\mouse_driver" "fsroot\mouse.elf" -Force
Copy-Item "userland\compositor\target\x86_64-unknown-none\release\compositor" "fsroot\comp.elf" -Force
Copy-Item "userland\terminal\target\x86_64-unknown-none\release\terminal" "fsroot\term.elf" -Force

Write-Host "Generating FAT12 Image..."
python make_fat.py build\fs.img fsroot\hello.elf fsroot\console.elf fsroot\keyboard.elf fsroot\init.elf fsroot\shell.elf fsroot\mouse.elf fsroot\comp.elf fsroot\term.elf

if (-not $SkipISO) {
    Write-Host "Re-creating ISO..."
    python build.py
}

if ($RunInQemu) {
    Write-Host "Running QEMU..."
    python build.py run-qemu
}

if (-not $RunInQemu) {
    Write-Host "Build Complete! You can now run QEMU manually."
}
