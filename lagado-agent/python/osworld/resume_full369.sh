#!/usr/bin/env bash
# resume_full369.sh — pick up the full-369 OSWorld campaign where it stopped.
# Resume-safe: lanes skip every task already in their results jsonl.
# Usage: ./resume_full369.sh    (start brain first: start_brain.sh, wait for :8080 health)
#
# THERMALS (user policy 2026-07-09, ceiling <85°C): each lane runs under a taskset core cap so
# the two nested VMs can't drive all 16 threads flat-out. Defaults leave 4 threads (2 cores)
# for the brain/desktop; override per lane: LAGADO_LANE_A_CPUS / LAGADO_LANE_B_CPUS.
# Run thermal_watch.sh alongside; on sustained ALERT, `kill -STOP $(cat full_?.pid)` (resume
# with -CONT) — the jsonl resume makes pausing free.
set -eu
OSW=/home/alucard/projects/OSWorld
RUN=/home/alucard/projects/lagado/lagado-agent/python/osworld/osworld_run.py
CPUS_a="${LAGADO_LANE_A_CPUS:-0-5}"       # lane a: cores 0-2 (HT pairs 0-5)
CPUS_b="${LAGADO_LANE_B_CPUS:-6-11}"      # lane b: cores 3-5 (HT pairs 6-11); 12-15 stay free
for LANE in a b; do
  CPUS_VAR="CPUS_${LANE}"
  # taskset caps the lane driver; LAGADO_CPUSET caps the CONTAINER (nested qemu) — the real
  # heat source, which is a child of the podman service and does NOT inherit the lane's mask.
  nohup taskset -c "${!CPUS_VAR}" bash -c "cd $OSW && while true; do \
    DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
    PYTHONPATH=$OSW \
    LAGADO_CPUSET=${!CPUS_VAR} \
    LAGADO_RESULTS=/tmp/lagado_battery/full_${LANE}.jsonl \
    .venv/bin/python -u $RUN \$(cat /tmp/lagado_battery/full_${LANE}.txt) && break; \
    echo LANE-RESTART; sleep 30; done" \
    > /tmp/lagado_battery/full_${LANE}.log 2>&1 &
  echo $! > /tmp/lagado_battery/full_${LANE}.pid
done
echo "lanes resumed (cpu caps a=$CPUS_a b=$CPUS_b) — progress: $(cat /tmp/lagado_battery/full_?.jsonl 2>/dev/null | wc -l)/369 already done"
