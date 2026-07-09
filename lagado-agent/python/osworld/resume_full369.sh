#!/usr/bin/env bash
# resume_full369.sh — pick up the full-369 OSWorld campaign where it stopped.
# Resume-safe: lanes skip every task already in their results jsonl.
# Usage: ./resume_full369.sh    (start brain first: start_brain.sh, wait for :8080 health)
set -eu
OSW=/home/alucard/projects/OSWorld
RUN=/home/alucard/projects/lagado/lagado-agent/python/osworld/osworld_run.py
for LANE in a b; do
  nohup bash -c "cd $OSW && while true; do \
    DOCKER_HOST=unix:///run/user/1000/podman/podman.sock \
    PYTHONPATH=$OSW \
    LAGADO_RESULTS=/tmp/lagado_battery/full_${LANE}.jsonl \
    .venv/bin/python -u $RUN \$(cat /tmp/lagado_battery/full_${LANE}.txt) && break; \
    echo LANE-RESTART; sleep 30; done" \
    > /tmp/lagado_battery/full_${LANE}.log 2>&1 &
  echo $! > /tmp/lagado_battery/full_${LANE}.pid
done
echo "lanes resumed — progress: $(cat /tmp/lagado_battery/full_?.jsonl 2>/dev/null | wc -l)/369 already done"
