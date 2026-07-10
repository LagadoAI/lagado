#!/usr/bin/env bash
# resume_full369.sh — pick up the full-369 OSWorld campaign where it stopped.
# Resume-safe: lanes skip every task already in their results jsonl.
# Usage: ./resume_full369.sh    (start brain first: start_brain.sh, wait for :8080 health)
#
# THERMALS (user policy 2026-07-09, ceiling <85°C): each lane's CONTAINER (the nested qemu, the
# real heat source) runs under a CPU-TIME quota via LAGADO_CPUS → provider nano_cpus (the `cpu`
# controller, which IS delegated rootless; cpuset is NOT — measured 2026-07-10). Default 3 cores
# of quota per lane (2 lanes = 6 of 16, leaving ample headroom for brain+desktop). Override:
# LAGADO_LANE_CPUS. Run thermal_watch.sh alongside; on sustained ALERT, `kill -STOP $(cat
# full_?.pid)` (resume with -CONT) — the jsonl resume makes pausing free.
set -eu
OSW=/home/alucard/projects/OSWorld
RUN=/home/alucard/projects/lagado/lagado-agent/python/osworld/osworld_run.py
LANE_CPUS="${LAGADO_LANE_CPUS:-3}"        # CPU-time quota (cores) per lane's container
for LANE in a b; do
  nohup bash -c "cd $OSW && while true; do \
    DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
    PYTHONPATH=$OSW \
    LAGADO_CPUS=$LANE_CPUS \
    LAGADO_RESULTS=/tmp/lagado_battery/full_${LANE}.jsonl \
    .venv/bin/python -u $RUN \$(cat /tmp/lagado_battery/full_${LANE}.txt) && break; \
    echo LANE-RESTART; sleep 30; done" \
    > /tmp/lagado_battery/full_${LANE}.log 2>&1 &
  echo $! > /tmp/lagado_battery/full_${LANE}.pid
done
echo "lanes resumed (cpu quota ${LANE_CPUS} cores/lane) — progress: $(cat /tmp/lagado_battery/full_?.jsonl 2>/dev/null | wc -l)/369 already done"
