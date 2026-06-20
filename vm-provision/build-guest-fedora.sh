#!/usr/bin/env bash
# build-guest-fedora.sh — reproduce the FEDORA 44 + CINNAMON Lagado guest from scratch.
#
# Sibling of build-guest.sh (Ubuntu). Produces, under $LAGADO_DATA_DIR/vm-images/:
#   - Fedora-Cloud-Base-Generic-44-*.qcow2   pristine Fedora 44 cloud base (downloaded once)
#   - lagado-guest-fedora.qcow2              working disk (base copy, grown to 20G)
#   - seed-fedora.iso                        cloud-init NoCloud seed (volid "cidata")
#
# Built as a SEPARATE artifact from the Ubuntu guest — it does NOT touch lagado-guest.qcow2, so a
# running Ubuntu VM keeps working. The guest is Cinnamon (Windows-like) on Fedora 44, GTK/AT-SPI2.
# First boot runs cloud-init (user-data-fedora) — several minutes (it dnf-installs a desktop).
# After it boots, prove the control channel with:  cargo run --bin harness_proof
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${LAGADO_DATA_DIR:-$HOME/.laputa-secure}"
IMG_DIR="$DATA_DIR/vm-images"
# Derive the exact base-image filename from the release dir (don't hardcode the build number).
# Use dl.fedoraproject.org (a direct mirror that serves a real directory listing) — the
# download.fedoraproject.org redirector returns no listing without -L.
REL_DIR="https://dl.fedoraproject.org/pub/fedora/linux/releases/44/Cloud/x86_64/images"
DISK="$IMG_DIR/lagado-guest-fedora.qcow2"
SEED="$IMG_DIR/seed-fedora.iso"
KEY="$HOME/.ssh/id_ed25519"

mkdir -p "$IMG_DIR"

# 1. Host keypair — the agent SSHes into the guest with this (BatchMode key auth). Shared with Ubuntu.
if [[ ! -f "$KEY" ]]; then
  echo "[build-guest-fedora] generating host keypair $KEY"
  ssh-keygen -t ed25519 -N "" -C "lagado-host" -f "$KEY"
fi
PUBKEY="$(cat "$KEY.pub")"

# 2. Cloud-init seed — substitute the real host pubkey into user-data-fedora, build cidata ISO.
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
sed "s|__HOST_PUBKEY__|$PUBKEY|" "$HERE/user-data-fedora" > "$TMP/user-data"
cp "$HERE/meta-data" "$TMP/meta-data"
genisoimage -output "$SEED" -volid cidata -joliet -rock "$TMP/user-data" "$TMP/meta-data"
echo "[build-guest-fedora] seed-fedora.iso built"

# 3. Base cloud image — resolve the current build's filename, download once.
# `|| true` so an empty/failed listing under pipefail+set -e doesn't abort before the fallback applies.
BASE_NAME="$(curl -sL --max-time 30 "$REL_DIR/" \
  | grep -oE 'Fedora-Cloud-Base-Generic-44-[0-9.]+\.x86_64\.qcow2' | sort -u | head -n1 || true)"
BASE_NAME="${BASE_NAME:-Fedora-Cloud-Base-Generic-44-1.7.x86_64.qcow2}"  # fallback to the known build
BASE="$IMG_DIR/$BASE_NAME"
if [[ ! -f "$BASE" ]]; then
  echo "[build-guest-fedora] downloading $BASE_NAME (~600 MB)…"
  wget -q -O "$BASE" "$REL_DIR/$BASE_NAME"
fi

# 4. Working disk = base copy grown to 20G (Cinnamon + apps need headroom).
cp -f "$BASE" "$DISK"
qemu-img resize "$DISK" 20G
echo "[build-guest-fedora] lagado-guest-fedora.qcow2 ready (20G)"
echo "[build-guest-fedora] boot it, let cloud-init finish (several min), then: cargo run --bin harness_proof"
