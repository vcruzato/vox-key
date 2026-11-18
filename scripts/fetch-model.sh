#!/usr/bin/env bash
# Downloads a quantized Whisper GGML model into voxkey's cache dir.
# Usage: scripts/fetch-model.sh [small|base|tiny|medium]
set -euo pipefail
MODEL="${1:-small}"
DIR="${XDG_CACHE_HOME:-$HOME/.cache}/voxkey"
FILE="ggml-${MODEL}-q5_1.bin"
mkdir -p "$DIR"
echo "Downloading $FILE from ggerganov/whisper.cpp …"
curl -fL "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$FILE" -o "$DIR/$FILE"
echo "Model ready: $DIR/$FILE"
