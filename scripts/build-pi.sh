#!/bin/sh
# Compile pour Raspberry Pi 5 (OS 64 bits, glibc 2.36 = Bookworm) depuis macOS.
# Prérequis : voir docs/install.md (rustup, zig, cargo-zigbuild, cible aarch64).
set -e
export PATH="$(brew --prefix rustup 2>/dev/null)/bin:$PATH"
TARGET=aarch64-unknown-linux-gnu
cargo zigbuild --release --target "$TARGET.2.36"
echo "Binaire : target/$TARGET/release/rpi-monitor"
