#!/bin/bash
set -e

export PATH="$HOME/.cargo/bin:/Users/huseyn/.cargo/bin:/opt/homebrew/bin:$PATH"

echo "Building Kernel..."
cargo build --release --target x86_64-unknown-none

build_userland() {
    echo "Building Userland ($1)..."
    cd "userland/$1"
    export RUSTFLAGS="-C relocation-model=static -C panic=abort"
    cargo build --release --target x86_64-unknown-none
    export RUSTFLAGS=""
    cd ../..
}

build_userland hello_user
build_userland console_driver
build_userland keyboard_driver
build_userland init
build_userland shell
build_userland mouse_driver
build_userland compositor
build_userland terminal

echo "Copying Userland binaries..."
mkdir -p fsroot
cp "userland/hello_user/target/x86_64-unknown-none/release/hello_user" "fsroot/hello.elf"
cp "userland/console_driver/target/x86_64-unknown-none/release/console_driver" "fsroot/console.elf"
cp "userland/keyboard_driver/target/x86_64-unknown-none/release/keyboard_driver" "fsroot/keyboard.elf"
cp "target/x86_64-unknown-none/release/init" "fsroot/init.elf" 2>/dev/null || cp "userland/init/target/x86_64-unknown-none/release/init" "fsroot/init.elf"
cp "target/x86_64-unknown-none/release/shell" "fsroot/shell.elf" 2>/dev/null || cp "userland/shell/target/x86_64-unknown-none/release/shell" "fsroot/shell.elf"
cp "userland/mouse_driver/target/x86_64-unknown-none/release/mouse_driver" "fsroot/mouse.elf"
cp "userland/compositor/target/x86_64-unknown-none/release/compositor" "fsroot/comp.elf"
cp "userland/terminal/target/x86_64-unknown-none/release/terminal" "fsroot/term.elf"

echo "Generating FAT12 Image..."
python3 make_fat.py build/fs.img fsroot/hello.elf fsroot/console.elf fsroot/keyboard.elf fsroot/init.elf fsroot/shell.elf fsroot/mouse.elf fsroot/comp.elf fsroot/term.elf

echo "Done running run_tests.sh"
