#!/usr/bin/env bash
# thermal_watch.sh — CPU + GPU temperature watch with an 85°C ceiling.
#
# Policy (user, 2026-07-09): 95°C was too high, 90°C still too high → the comfort ceiling is
# <85°C. The GPU was previously UNMEASURED because the RTX 3060 Laptop never appears in
# `sensors` output — it must be read via nvidia-smi. This watch reads BOTH every sample.
#
# Usage:  ./thermal_watch.sh [interval_s=30]
#   stdout: one line per sample "HH:MM:SS CPU=61C GPU=54C"; breaches append " ALERT@85C".
#   LAGADO_TEMP_LIMIT overrides the ceiling.
#
# Pair with a log-watcher that filters ALERT lines; on sustained alert, pause campaign lanes
# (kill -STOP the lane PIDs from /tmp/lagado_battery/full_?.pid) rather than killing them —
# the jsonl resume makes STOP/CONT the cheaper lever.
set -u
LIMIT="${LAGADO_TEMP_LIMIT:-85}"
INTERVAL="${1:-30}"
while true; do
  cpu=$(sensors 2>/dev/null | awk '/Package id 0/{gsub(/[+°C.]/,"",$4); print int($4/10); exit}')
  # sensors prints "+61.0°C" — the awk above strips to "610" then /10. Fall back to raw parse.
  if [ -z "${cpu:-}" ]; then
    cpu=$(sensors 2>/dev/null | grep -m1 'Package id 0' | grep -oE '\+[0-9]+' | head -1 | tr -d '+')
  fi
  gpu=$(nvidia-smi --query-gpu=temperature.gpu --format=csv,noheader,nounits 2>/dev/null | head -1 | tr -d ' ')
  line="$(date +%H:%M:%S) CPU=${cpu:-?}C GPU=${gpu:-?}C"
  if [ "${cpu:-0}" -ge "$LIMIT" ] 2>/dev/null || [ "${gpu:-0}" -ge "$LIMIT" ] 2>/dev/null; then
    line="$line ALERT@${LIMIT}C"
  fi
  echo "$line"
  sleep "$INTERVAL"
done
