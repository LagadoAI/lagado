#!/bin/bash
set -e
export LD_LIBRARY_PATH="$HOME/laputa/bin:$LD_LIBRARY_PATH"
BRIDGE="/dev/shm/laputa"
mkdir -p "$BRIDGE"


# ── Resume from frozen session if marker exists ─────────────────────────────
if [ -f "$BRIDGE/freeze_marker" ]; then
    echo "  Resuming from frozen session…"
    rm "$BRIDGE/freeze_marker"
fi

echo "══════════════════════════════════════════════"
echo " "  INITIALIZING LAPUTA COMMAND INTERFACE
echo "══════════════════════════════════════════════"

# 1. Engine (Qwen3-8B, GPU)
echo "[1/4] Cortex successfully mounted into ceberal matrix..."
pkill -f "llama-server.*8080" 2>/dev/null || true
~/laputa/scripts/run_cortex.sh > "$BRIDGE/engine.log" 2>&1 &
for i in {1..20}; do
    if curl -s http://127.0.0.1:8080/health | grep -q "ok"; then
        break
    fi
    sleep 2
done
echo "  Cortex: $(curl -s http://127.0.0.1:8080/health)"
if ! curl -s http://127.0.0.1:8080/health | grep -q "ok"; then
    echo "  ERROR: System failed to mount. Check $BRIDGE/engine.log"
    exit 1
fi

# 2. Background services
echo "[2/4] Engaging parasympathetic services..."
pkill -f governor.py 2>/dev/null || true
python3 ~/laputa/scripts/governor.py > "$BRIDGE/governor.log" 2>&1 &
pkill -f thalamus.py 2>/dev/null || true
python3 ~/laputa/scripts/thalamus.py > "$BRIDGE/thalamus.log" 2>&1 &
pkill -f vault_ingest.py 2>/dev/null || true
python3 ~/laputa/scripts/vault_ingest.py > "$BRIDGE/vault_ingest.log" 2>&1 &
echo "  Core systems engaged"

# 3. Launch Laputa Desktop (always — the user starts in Chat mode)
echo "[3/4] **Begin Activation**"
cd ~/laputa/laputa-ui
WAYLAND_DISPLAY=wayland-0 pnpm tauri dev &

# 4. Auxiliary systems (LightOnOCR for vault processing)
echo "[4/4] Auxiliary systems activated"
pkill -f "llama-server.*8082" 2>/dev/null || true
~/laputa/scripts/run_eyes.sh > "$BRIDGE/eyes.log" 2>&1 &
sleep 2
echo "  Vision: $(curl -s http://127.0.0.1:8082/health 2>/dev/null || echo starting...)"
echo "  Connection established..."
# ── Animation (interactive terminals only) ──────────────────────────────────
[[ -t 1 ]] && python3 ~/laputa/scripts/arise_animation.py
