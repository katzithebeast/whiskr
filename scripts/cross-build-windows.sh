#!/usr/bin/env bash
# Cross-compile the Windows installer from Linux (no Windows machine needed).
# Needs: mingw-w64, nsis, Node, Rust. If `rustup target add x86_64-pc-windows-gnu`
# works on your network, the RUSTC_BOOTSTRAP/-Zbuild-std part is unnecessary.
set -euo pipefail
cd "$(dirname "$0")/.."
export CARGO_TARGET_X86_64_PC_WINDOWS_GNU_LINKER=x86_64-w64-mingw32-gcc
export RUSTC_BOOTSTRAP=1
npx tauri build --target x86_64-pc-windows-gnu --bundles nsis -- -Zbuild-std=std,panic_abort
echo "Installer: src-tauri/target/x86_64-pc-windows-gnu/release/bundle/nsis/"
