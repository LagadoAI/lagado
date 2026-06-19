#!/usr/bin/env bash
# Lagado AI — dev launcher (also the target of the desktop/app-icon entry; see install-desktop.sh)
set -e

REPO="$(cd "$(dirname "$0")" && pwd)"
LLAMA_BUILD="$REPO/lagado-agent/vendored/llama.cpp-2/build/bin"

# When launched from a GUI app icon (not a shell), PATH can be minimal or EMPTY — so node/npm (at
# /usr/bin) and cargo aren't found and `exec npm` dies silently. Prepend cargo + the standard system
# bin dirs unconditionally so the launch works regardless of the desktop launcher's environment.
[ -f "$HOME/.cargo/env" ] && . "$HOME/.cargo/env"
export PATH="$HOME/.cargo/bin:/usr/local/bin:/usr/bin:/bin:$PATH"

export WEBKIT_DISABLE_DMABUF_RENDERER=1

# Keep the UI OFF the NVIDIA dGPU so the whole 6GB card is free for the 8B brain (the design: brain on
# GPU, everything else off it). The WebKitGTK webview was grabbing a ~1GB GL context on the 3060.
# Pinning GLX/EGL to Mesa FAILED here ("failed to create dri2 screen"), so disable WebKit's accelerated
# compositing entirely — it then renders without a GPU context (a little more CPU, but frees ~1GB VRAM,
# the right trade on a card this size). Belt-and-suspenders: also nudge GLX/PRIME toward the iGPU.
export WEBKIT_DISABLE_COMPOSITING_MODE=1
export __NV_PRIME_RENDER_OFFLOAD=0
export __GLX_VENDOR_LIBRARY_NAME=mesa

export LAGADO_DATA_DIR="$HOME/.laputa-secure"
export LAGADO_LLAMA_SERVER="$LLAMA_BUILD/llama-server"
export LD_LIBRARY_PATH="$LLAMA_BUILD${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

cd "$REPO/lagado-ui"
exec npm run tauri dev
