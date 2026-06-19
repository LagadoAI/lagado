#!/usr/bin/env bash
# Lagado AI — dev launcher
set -e

REPO="$(cd "$(dirname "$0")" && pwd)"
LLAMA_BUILD="$REPO/lagado-agent/vendored/llama.cpp-2/build/bin"

export WEBKIT_DISABLE_DMABUF_RENDERER=1

# Render the UI on the Intel iGPU (TigerLake), NOT the NVIDIA dGPU — the WebKitGTK webview was
# grabbing a GL/EGL context on the 3060 (~1GB VRAM), starving the inference engine on a 6GB card.
# Pin GLX/EGL to Mesa and disable PRIME offload so the whole dGPU stays free for llama.cpp/CUDA.
export __NV_PRIME_RENDER_OFFLOAD=0
export __GLX_VENDOR_LIBRARY_NAME=mesa
export __EGL_VENDOR_LIBRARY_FILENAMES=/usr/share/glvnd/egl_vendor.d/50_mesa.json

export LAGADO_DATA_DIR="$HOME/.laputa-secure"
export LAGADO_LLAMA_SERVER="$LLAMA_BUILD/llama-server"
export LD_LIBRARY_PATH="$LLAMA_BUILD${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

cd "$REPO/lagado-ui"
exec npm run tauri dev
