#!/usr/bin/env bash
# BRUTAL settle-monitor weakness tests (2026-07-06 directive: "the most brutal tests
# you can come up with to show its weakness"). Four phases, each attacking a different
# failure surface. Run AFTER the vm30 sweep frees the VM. Output -> brutal_settle.log.
#
#  A  SERVICE-KILL   kill settle_service every 1s during the run  -> fail-open MUST
#                    stand in (mode=cfc_failopen, floor sleep) and the task still gold.
#  B  RENDER N=3     aa3a8974 6e99a1ad a01fbce3 x3 rounds -> early-release variance hunt
#                    on the render-sensitive class (the seam the monitor guards).
#  C  AMBIENT CHURN  inject a continuously-painting window (xterm+top via the guest's
#                    /execute) right before evaluate -> monitor must NOT settle early;
#                    watch for 15s cap-outs (latency cost) and any dropped gold.
#  D  FORCED 2s CAP  LAGADO_SETTLE_MAX=2 (below the observed ~2.7s honest settle) ->
#                    synthetic early release. If golds survive, the seam has slack;
#                    if they drop, release timing is proven load-bearing (ablation prior).
set -u
export DOCKER_HOST=unix:///run/user/1000/podman/podman.sock
export PYTHONPATH=/home/alucard/projects/OSWorld:/home/alucard/projects/lagado/lagado-agent/python/osworld
OSW=/home/alucard/projects/OSWorld
PY=$OSW/.venv/bin/python
BAT=/home/alucard/projects/lagado/lagado-agent/python/osworld/battery_breadth.py
AUDIT=/home/alucard/projects/lagado/lagado-agent/python/osworld/settle_audit.py
LOGS=/tmp/lagado_battery/breadth_logs.jsonl
cd $OSW

clean() { podman rm -f $(podman ps -aq) >/dev/null 2>&1; podman volume prune -f >/dev/null 2>&1; }
mark()  { wc -l < $LOGS 2>/dev/null || echo 0; }
audit_since() { local n=$1; local total=$(mark); python3 $AUDIT --tail=$((total - n)); }

echo "=================== PHASE A: SERVICE-KILL (fail-open) ==================="
clean
A0=$(mark)
( while true; do pkill -f "settle_servic[e].py" 2>/dev/null; sleep 1; done ) &
KILLER=$!
$PY -u $BAT aa3a8974
kill $KILLER 2>/dev/null
echo "--- Phase A audit (expect mode=cfc_failopen, score still 1.0) ---"
audit_since $A0

echo "=================== PHASE B: RENDER-CLASS N=3 ==================="
B0=$(mark)
for r in 1 2 3; do
  echo "--- render round $r ---"
  clean
  $PY -u $BAT aa3a8974 6e99a1ad a01fbce3
done
echo "--- Phase B audit (variance + early-release hunt) ---"
audit_since $B0

echo "=================== PHASE C: AMBIENT CHURN ==================="
clean
C0=$(mark)
# churn injector: waits for the task container's 5000->host port, then launches a
# continuously-repainting window inside the guest so the screen NEVER goes pixel-quiet.
(
  for i in $(seq 1 90); do
    PORT=$(podman ps --format '{{.Ports}}' 2>/dev/null | grep -oP '\d+(?=->5000)' | head -1)
    [ -n "${PORT:-}" ] && break; sleep 2
  done
  [ -z "${PORT:-}" ] && exit 0
  sleep 45   # let setup finish; churn lands mid-task, before the settle seam
  curl -s -m 10 -X POST http://localhost:$PORT/execute -H 'Content-Type: application/json' \
    -d '{"command":"DISPLAY=:0 xterm -geometry 80x24+50+50 -e top &","shell":true}' >/dev/null 2>&1
) &
CHURN=$!
$PY -u $BAT aa3a8974
kill $CHURN 2>/dev/null
echo "--- Phase C audit (expect LONGER settles / cap-outs, NO early release, gold held) ---"
audit_since $C0

echo "=================== PHASE D: FORCED 2s CAP (seam probe) ==================="
clean
D0=$(mark)
LAGADO_SETTLE_MAX=2 $PY -u $BAT aa3a8974 6e99a1ad a01fbce3
echo "--- Phase D audit (synthetic early release: do golds survive a 2s exit?) ---"
audit_since $D0

clean
echo BRUTAL-SETTLE-DONE
