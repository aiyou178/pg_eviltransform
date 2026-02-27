#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
OUT_DIR="${OUT_DIR:-$ROOT_DIR/dist}"
EXT_VERSION="${EXT_VERSION:-$(awk -F'"' '/^version = / { print $2; exit }' "$ROOT_DIR/Cargo.toml")}"
PG_VERSIONS=(14 15 16 17 18)

mkdir -p "$OUT_DIR"

cd "$ROOT_DIR"

# Initialize pgrx once using PG18. Per-version packaging passes explicit --pg-config.
if [[ ! -x /usr/lib/postgresql/18/bin/pg_config ]]; then
  echo "missing /usr/lib/postgresql/18/bin/pg_config" >&2
  exit 2
fi
cargo pgrx init --pg18=/usr/lib/postgresql/18/bin/pg_config

for pg in "${PG_VERSIONS[@]}"; do
  echo "[package] PostgreSQL $pg"
  pg_config="/usr/lib/postgresql/$pg/bin/pg_config"
  if [[ ! -x "$pg_config" ]]; then
    echo "missing pg_config for PostgreSQL $pg at $pg_config" >&2
    exit 2
  fi

  build_dir="$ROOT_DIR/target/pgrx-pkg/pg$pg"
  rm -rf "$build_dir"
  mkdir -p "$build_dir"

  cargo pgrx package \
    --release \
    --features "pg$pg" \
    --no-default-features \
    --pg-config "$pg_config" \
    --out-dir "$build_dir"

  package_root="$(find "$build_dir" -maxdepth 1 -type d -name 'pg_eviltransform-pg*' | head -n 1)"
  if [[ -z "$package_root" ]]; then
    echo "failed to find cargo pgrx package output for PostgreSQL $pg" >&2
    exit 1
  fi

  deb_root="$build_dir/deb"
  rm -rf "$deb_root"
  mkdir -p "$deb_root/DEBIAN"
  cp -a "$package_root/usr" "$deb_root/"

  control_path="$deb_root/usr/share/postgresql/$pg/extension/pg_eviltransform.control"
  if [[ -f "$control_path" ]]; then
    cp "$ROOT_DIR/src/pg_eviltransform.control" "$control_path"
  fi

  cat > "$deb_root/DEBIAN/control" <<CONTROL
Package: postgresql-${pg}-pg-eviltransform
Version: ${EXT_VERSION}
Section: database
Priority: optional
Architecture: amd64
Maintainer: Open Source <opensource@example.com>
Depends: postgresql-${pg}, postgresql-${pg}-postgis-3
Description: transformation of bd09, gcj02 and other coordinate supported by postgis ST_Transform
 Rust+pgrx extension for coordinate transformation using WGS84 (SRID 4326) as intermediary.
CONTROL

  out_deb="$OUT_DIR/postgresql-${pg}-pg-eviltransform_${EXT_VERSION}_trixie_amd64.deb"
  dpkg-deb --build "$deb_root" "$out_deb"
  echo "[package] wrote $out_deb"
done
