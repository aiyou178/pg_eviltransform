#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="${IMAGE_NAME:-pg_eviltransform/trixie-builder:local}"
EXT_VERSION="${1:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
OUT_DIR="${2:-$ROOT_DIR/dist}"
OUT_DIR="$(cd "$(dirname "$OUT_DIR")" && pwd)/$(basename "$OUT_DIR")"

mkdir -p "$OUT_DIR"

echo "[release] building Docker image on Debian trixie"
docker build -f "$ROOT_DIR/docker/Dockerfile.release-trixie" -t "$IMAGE_NAME" "$ROOT_DIR"

echo "[release] packaging pg_eviltransform version $EXT_VERSION for PostgreSQL 14-18"
docker run --rm \
  -e EXT_VERSION="$EXT_VERSION" \
  -e OUT_DIR="/out" \
  -v "$ROOT_DIR:/work" \
  -v "$OUT_DIR:/out" \
  "$IMAGE_NAME" \
  /work/scripts/package_debs.sh

echo "[release] done; artifacts in $OUT_DIR"
