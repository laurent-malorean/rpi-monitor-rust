#!/bin/sh
# Compile pour Raspberry Pi 5 (OS 64 bits, glibc 2.36 = Bookworm) depuis macOS.
# Résultat : dist/rpi-monitor, le seul fichier à copier sur le Pi.
# Les fichiers intermédiaires de cargo vont dans un cache hors du projet.
# Prérequis : voir docs/install.md (rustup, zig, cargo-zigbuild, cible aarch64).
set -e
cd "$(dirname "$0")/.."
export PATH="$(brew --prefix rustup 2>/dev/null)/bin:$PATH"
export CARGO_TARGET_DIR="${HOME}/.cache/rpi-monitor-target"
TARGET=aarch64-unknown-linux-gnu
cargo zigbuild --release --target "$TARGET.2.36"
mkdir -p dist
cp "$CARGO_TARGET_DIR/$TARGET/release/rpi-monitor" dist/rpi-monitor
echo "Binaire : dist/rpi-monitor ($(du -h dist/rpi-monitor | cut -f1))"
