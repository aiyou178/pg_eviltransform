#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/dist}"
EXT_VERSION="${EXT_VERSION:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
PG_VERSIONS="${PG_VERSIONS:-14 15 16 17 18}"
ARCH="$(dpkg --print-architecture)"
read -r -a PG_VERSION_LIST <<< "$PG_VERSIONS"

if [[ ${#PG_VERSION_LIST[@]} -eq 0 ]]; then
  echo "PG_VERSIONS must contain at least one PostgreSQL major version" >&2
  exit 2
fi

mkdir -p "$OUT_DIR"

cd "$ROOT_DIR"

init_args=()
for pg in "${PG_VERSION_LIST[@]}"; do
  if [[ ! "$pg" =~ ^(14|15|16|17|18|19)$ ]]; then
    echo "unsupported PostgreSQL major version in PG_VERSIONS: $pg" >&2
    exit 2
  fi
  pg_config="/usr/lib/postgresql/$pg/bin/pg_config"
  if [[ ! -x "$pg_config" ]]; then
    echo "missing pg_config for PostgreSQL $pg at $pg_config" >&2
    exit 2
  fi
  init_args+=("--pg${pg}=${pg_config}")
done

cargo pgrx init "${init_args[@]}"

for pg in "${PG_VERSION_LIST[@]}"; do
  echo "[package] PostgreSQL $pg"
  pg_config="/usr/lib/postgresql/$pg/bin/pg_config"
  cargo pgrx install \
    -v \
    --release \
    --features "pg$pg" \
    --no-default-features \
    --pg-config "$pg_config"

  ext_dir="/usr/share/postgresql/$pg/extension"
  lib_dir="/usr/lib/postgresql/$pg/lib"
  if [[ ! -d "$ext_dir" || ! -d "$lib_dir" ]]; then
    echo "missing extension/lib install dirs for PostgreSQL $pg" >&2
    exit 1
  fi

  shopt -s nullglob
  sql_files=("$ext_dir"/pg_eviltransform--*.sql)
  if [[ ${#sql_files[@]} -eq 0 ]]; then
    echo "no SQL files generated in $ext_dir for PostgreSQL $pg" >&2
    exit 1
  fi

  build_dir="$ROOT_DIR/target/pgrx-pkg/pg$pg"
  deb_root="$build_dir/deb"
  rm -rf "$deb_root"
  mkdir -p \
    "$deb_root/DEBIAN" \
    "$deb_root/usr/share/postgresql/$pg/extension" \
    "$deb_root/usr/lib/postgresql/$pg/lib"

  cp "$ROOT_DIR/src/pg_eviltransform.control" "$deb_root/usr/share/postgresql/$pg/extension/pg_eviltransform.control"
  cp "$lib_dir/pg_eviltransform.so" "$deb_root/usr/lib/postgresql/$pg/lib/"
  cp "${sql_files[@]}" "$deb_root/usr/share/postgresql/$pg/extension/"

  upgrade_files=("$ext_dir"/pg_eviltransform--*--*.sql)
  if [[ ${#upgrade_files[@]} -gt 0 ]]; then
    cp "${upgrade_files[@]}" "$deb_root/usr/share/postgresql/$pg/extension/"
  fi

  cat > "$deb_root/DEBIAN/control" <<CONTROL
Package: postgresql-${pg}-pg-eviltransform
Version: ${EXT_VERSION}
Section: database
Priority: optional
Architecture: ${ARCH}
Maintainer: Liang Zhanzhao <liangzhanzhao1985@gmail.com>
Suggests: postgresql-${pg}, postgresql-${pg}-postgis-3
Description: transformation of bd09, gcj02 and other coordinate supported by postgis ST_Transform
 Rust+pgrx extension for coordinate transformation using WGS84 (SRID 4326) as intermediary.
CONTROL

  out_deb="$OUT_DIR/postgresql-${pg}-pg-eviltransform_${EXT_VERSION}_trixie_${ARCH}.deb"
  dpkg-deb --build "$deb_root" "$out_deb"
  echo "[package] wrote $out_deb"
done
