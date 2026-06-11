#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
IMAGE_NAME="${IMAGE_NAME:-pg_eviltransform/trixie-builder:local}"
EXT_VERSION="${1:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
OUT_DIR="${2:-$ROOT_DIR/dist}"
OUT_DIR="$(cd "$(dirname "$OUT_DIR")" && pwd)/$(basename "$OUT_DIR")"
PG_VERSIONS="${PG_VERSIONS:-14 15 16 17 18}"
INCLUDE_PG19_BETA=0
BASE_IMAGE_SET="${BASE_IMAGE+x}"

if [[ " $PG_VERSIONS " == *" 19 "* ]]; then
  INCLUDE_PG19_BETA=1
  if [[ -z "$BASE_IMAGE_SET" ]]; then
    BASE_IMAGE="postgres:19beta1-trixie"
  fi
elif [[ -z "$BASE_IMAGE_SET" ]]; then
  BASE_IMAGE="postgres:18.3-trixie"
fi

mkdir -p "$OUT_DIR"

echo "[release] building Docker image on Debian trixie"
build_args=(
  --build-arg "BASE_IMAGE=$BASE_IMAGE"
  --build-arg "INCLUDE_PG19_BETA=$INCLUDE_PG19_BETA"
)
if [[ -n "${DEBIAN_MIRROR:-}" ]]; then
  build_args+=(--build-arg "DEBIAN_MIRROR=$DEBIAN_MIRROR")
fi
if [[ -n "${DEBIAN_SECURITY_MIRROR:-}" ]]; then
  build_args+=(--build-arg "DEBIAN_SECURITY_MIRROR=$DEBIAN_SECURITY_MIRROR")
fi
if [[ -n "${PGDG_MIRROR:-}" ]]; then
  build_args+=(--build-arg "PGDG_MIRROR=$PGDG_MIRROR")
fi
if [[ -n "${RUSTUP_DIST_SERVER:-}" ]]; then
  build_args+=(--build-arg "RUSTUP_DIST_SERVER=$RUSTUP_DIST_SERVER")
fi
if [[ -n "${RUSTUP_UPDATE_ROOT:-}" ]]; then
  build_args+=(--build-arg "RUSTUP_UPDATE_ROOT=$RUSTUP_UPDATE_ROOT")
fi
if [[ -n "${CARGO_REGISTRIES_CRATES_IO_INDEX:-}" ]]; then
  build_args+=(--build-arg "CARGO_REGISTRIES_CRATES_IO_INDEX=$CARGO_REGISTRIES_CRATES_IO_INDEX")
fi

docker build \
  "${build_args[@]}" \
  -f "$ROOT_DIR/docker/Dockerfile.release-trixie" \
  -t "$IMAGE_NAME" \
  "$ROOT_DIR"

echo "[release] packaging pg_eviltransform version $EXT_VERSION for PostgreSQL $PG_VERSIONS"
docker run --rm \
  -e EXT_VERSION="$EXT_VERSION" \
  -e OUT_DIR="/out" \
  -e PG_VERSIONS="$PG_VERSIONS" \
  -v "$ROOT_DIR:/work" \
  -v "$OUT_DIR:/out" \
  "$IMAGE_NAME" \
  /work/scripts/package_debs.sh

echo "[release] done; artifacts in $OUT_DIR"
