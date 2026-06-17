#!/usr/bin/env bash
# build-guest.sh — reproduce the Lagado guest VM from scratch (disaster recovery).
#
# Produces, under $LAGADO_DATA_DIR/vm-images/ (default ~/.laputa-secure/vm-images):
#   - noble-server-cloudimg-amd64.img   pristine Ubuntu 24.04 cloud base (downloaded once)
#   - lagado-guest.qcow2                 working disk (base copy, grown to 20G)
#   - seed.iso                           cloud-init NoCloud seed (volid "cidata")
#
# The guest is the agent's working surface: XFCE + autologin + SSH (host key auth) +
# AT-SPI2 + xdotool + tine. First boot runs cloud-init (user-data here) — several minutes.
# After it boots, prove the control channel with:  cargo run --bin harness_proof
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
DATA_DIR="${LAGADO_DATA_DIR:-$HOME/.laputa-secure}"
IMG_DIR="$DATA_DIR/vm-images"
BASE_URL="https://cloud-images.ubuntu.com/noble/current/noble-server-cloudimg-amd64.img"
BASE="$IMG_DIR/noble-server-cloudimg-amd64.img"
DISK="$IMG_DIR/lagado-guest.qcow2"
SEED="$IMG_DIR/seed.iso"
KEY="$HOME/.ssh/id_ed25519"

mkdir -p "$IMG_DIR"

# 1. Host keypair — the agent SSHes into the guest with this (BatchMode key auth).
if [[ ! -f "$KEY" ]]; then
  echo "[build-guest] generating host keypair $KEY"
  ssh-keygen -t ed25519 -N "" -C "lagado-host" -f "$KEY"
fi
PUBKEY="$(cat "$KEY.pub")"

# 2. Cloud-init seed — substitute the real host pubkey into user-data, build cidata ISO.
TMP="$(mktemp -d)"; trap 'rm -rf "$TMP"' EXIT
sed "s|__HOST_PUBKEY__|$PUBKEY|" "$HERE/user-data" > "$TMP/user-data"
cp "$HERE/meta-data" "$TMP/meta-data"
genisoimage -output "$SEED" -volid cidata -joliet -rock "$TMP/user-data" "$TMP/meta-data"
echo "[build-guest] seed.iso built"

# 3. Base cloud image (downloaded once) + working disk grown to 20G.
if [[ ! -f "$BASE" ]]; then
  echo "[build-guest] downloading Ubuntu 24.04 cloud base (~600 MB)…"
  wget -q -O "$BASE" "$BASE_URL"
fi
cp -f "$BASE" "$DISK"
qemu-img resize "$DISK" 20G
echo "[build-guest] lagado-guest.qcow2 ready (20G)"
echo "[build-guest] done. Prove with: LAGADO_DATA_DIR=$DATA_DIR cargo run --bin harness_proof"
