#!/usr/bin/env bash
# Lagado AI — dev launcher
set -e

REPO="$(cd "$(dirname "$0")" && pwd)"
LLAMA_BUILD="$REPO/lagado-agent/vendored/llama.cpp-2/build/bin"

export WEBKIT_DISABLE_DMABUF_RENDERER=1
export LAGADO_DATA_DIR="$HOME/.laputa-secure"
export LAGADO_LLAMA_SERVER="$LLAMA_BUILD/llama-server"
export LD_LIBRARY_PATH="$LLAMA_BUILD${LD_LIBRARY_PATH:+:$LD_LIBRARY_PATH}"

cd "$REPO/lagado-ui"
exec npm run tauri dev
