#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPORT_PATH="${1:-$ROOT_DIR/benchmark_pg18_report.txt}"
ROWS="${ROWS:-200000}"
PGHOST="${PGHOST:-127.0.0.1}"
PGPORT="${PGPORT:-5432}"
PGUSER="${PGUSER:-postgres}"
PGDATABASE="${PGDATABASE:-postgres}"

export PGHOST PGPORT PGUSER PGDATABASE

cat > /tmp/benchmark_pg18.sql <<SQL
\\timing on
SET client_min_messages TO warning;
SET jit = off;

CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION IF NOT EXISTS pg_eviltransform;

DO \$\$
BEGIN
  IF current_setting('server_version_num')::int < 180000
     OR current_setting('server_version_num')::int >= 190000 THEN
    RAISE EXCEPTION 'benchmark_pg18.sh requires PostgreSQL 18, current version_num=%',
      current_setting('server_version_num');
  END IF;
END
\$\$;

\\echo loading regex_eviltransform.sql
\\i $ROOT_DIR/regex_eviltransform.sql

DROP TABLE IF EXISTS public.bench_points;
CREATE TABLE public.bench_points AS
SELECT
  id,
  ST_SetSRID(
    ST_MakePoint(
      72.004 + random() * (137.8347 - 72.004),
      0.8293 + random() * (55.8271 - 0.8293)
    ),
    4326
  ) AS geom
FROM generate_series(1, ${ROWS}) AS id;

ANALYZE public.bench_points;

\\echo baseline check
SELECT count(*) AS total_rows FROM public.bench_points;

\\echo benchmark: 4326 -> 990001
EXPLAIN (ANALYZE, BUFFERS)
SELECT sum(ST_X(g) + ST_Y(g))
FROM (
  SELECT ST_EvilTransform(geom, 990001) AS g
  FROM public.bench_points
) t;

EXPLAIN (ANALYZE, BUFFERS)
SELECT sum(ST_X(g) + ST_Y(g))
FROM (
  SELECT Regex_EvilTransform(geom, 990001) AS g
  FROM public.bench_points
) t;

\\echo benchmark: 990002 -> 3857 (via 4326)
EXPLAIN (ANALYZE, BUFFERS)
SELECT sum(ST_X(g) + ST_Y(g))
FROM (
  SELECT ST_EvilTransform(ST_EvilTransform(geom, 990002), 3857) AS g
  FROM public.bench_points
) t;

EXPLAIN (ANALYZE, BUFFERS)
SELECT sum(ST_X(g) + ST_Y(g))
FROM (
  SELECT Regex_EvilTransform(Regex_EvilTransform(geom, 990002), 3857) AS g
  FROM public.bench_points
) t;
SQL

psql -v ON_ERROR_STOP=1 -f /tmp/benchmark_pg18.sql | tee "$REPORT_PATH"

echo "Benchmark report written to $REPORT_PATH"
