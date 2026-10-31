#!/bin/sh
# Compile pour Raspberry Pi 5 (OS 64 bits). Nécessite `cross` (Docker) ou `cargo-zigbuild` (+ zig).
set -e
TARGET=aarch64-unknown-linux-gnu
if command -v cross >/dev/null; then
  cross build --release --target "$TARGET"
elif command -v cargo-zigbuild >/dev/null; then
  cargo zigbuild --release --target "$TARGET"
else
  echo "Installer 'cross' (cargo install cross) ou 'cargo-zigbuild'." >&2; exit 1
fi
echo "Binaire : target/$TARGET/release/rpi-monitor"
