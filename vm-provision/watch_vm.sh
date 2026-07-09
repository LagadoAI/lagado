#!/usr/bin/env bash
# watch_vm.sh — boot the lagado guest VISIBLY (user directive 2026-07-08: "I want to
# watch from now on"). A native GTK window shows the guest on the desktop while the
# SAME instance serves VNC :7 for the membrane feed and QMP for input/screendump —
# you watch exactly what the agent sees and does, live.
#
# Usage: ./watch_vm.sh [image]   (snapshot mode — the disk is never modified)
set -eu
IMG="${1:-$HOME/.laputa-secure/vm-images/lagado-guest-fedora.qcow2}"
rm -f /dev/shm/lg_ram
exec qemu-system-x86_64 -enable-kvm -m 2048 \
  -object memory-backend-file,id=m0,size=2048M,mem-path=/dev/shm/lg_ram,share=on \
  -machine q35,memory-backend=m0 \
  -device virtio-vga \
  -display gtk,show-cursor=on -vnc 127.0.0.1:7 \
  -device qemu-xhci -device usb-tablet \
  -qmp unix:/tmp/lg_qmp.sock,server,nowait \
  -drive "file=$IMG,if=virtio,snapshot=on" \
  -netdev user,id=n0 -device virtio-net-pci,netdev=n0 \
  -name "Lagado Guest (watch mode)" \
  -pidfile /tmp/lg_qemu.pid
