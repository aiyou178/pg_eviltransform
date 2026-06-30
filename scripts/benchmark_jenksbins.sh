#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${1:-$ROOT_DIR/benchmark_jenksbins_report.txt}"
ROWS="${ROWS:-1000000}"
DISTINCT_VALUES="${DISTINCT_VALUES:-10000}"
BREAKS="${BREAKS:-7}"
WORK_MEM="${WORK_MEM:-16MB}"
CDB_JENKSBINS_SQL="${CDB_JENKSBINS_SQL:-$ROOT_DIR/scripts/CDB_JenksBins.sql}"

cat > "/tmp/benchmark_jenksbins.sql" <<SQL
\\timing on
\\pset pager off

CREATE EXTENSION IF NOT EXISTS pg_eviltransform;

SET work_mem = :'work_mem';

DROP TABLE IF EXISTS public.bench_jenks_values;
CREATE TABLE public.bench_jenks_values AS
SELECT ((g - 1) % :distinct_values)::double precision
       + ((((g::bigint * 1103515245 + 12345) % 1000))::double precision / 1000.0) AS value
FROM generate_series(1, :rows) AS g;

ANALYZE public.bench_jenks_values;

\\echo benchmark: CartoDB CDB_JenksBins(array_agg(value::numeric), breaks)
EXPLAIN (ANALYZE, BUFFERS, TIMING)
SELECT CDB_JenksBins(array_agg(value::numeric), :breaks)
FROM public.bench_jenks_values;

\\echo benchmark: Rust ST_JenksBins(array_agg(value), breaks)
EXPLAIN (ANALYZE, BUFFERS, TIMING)
SELECT ST_JenksBins(array_agg(value), :breaks)
FROM public.bench_jenks_values;

\\echo benchmark: Rust streaming ST_JenksBins(value, breaks)
EXPLAIN (ANALYZE, BUFFERS, TIMING)
SELECT ST_JenksBins(value, :breaks)
FROM public.bench_jenks_values;
SQL

baseline_sql="$(mktemp)"
cleanup() {
  rm -f "$baseline_sql" "/tmp/benchmark_jenksbins.sql"
}
trap cleanup EXIT

{
  sed 's/@extschema@/public/g' "$CDB_JENKSBINS_SQL"
} > "$baseline_sql"

psql -v ON_ERROR_STOP=1 -f "$baseline_sql"
psql \
  -v ON_ERROR_STOP=1 \
  -v rows="$ROWS" \
  -v distinct_values="$DISTINCT_VALUES" \
  -v breaks="$BREAKS" \
  -v work_mem="$WORK_MEM" \
  -f "/tmp/benchmark_jenksbins.sql" 2>&1 | tee "$REPORT_PATH"
