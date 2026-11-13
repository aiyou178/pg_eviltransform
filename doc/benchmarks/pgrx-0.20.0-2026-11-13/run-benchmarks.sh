#!/usr/bin/env bash
set -euo pipefail
phase="$1"
major="$2"
export PGHOST=/tmp PGPORT="54${major}" PGUSER=postgres PGDATABASE=postgres PG_MAJOR="$major"
export PATH="/usr/lib/postgresql/${major}/bin:$PATH"
result_dir="/tmp/bench-results/${phase}-pg${major}"
mkdir -p "$result_dir"
cd /work
psql -v ON_ERROR_STOP=1 -c 'CREATE EXTENSION IF NOT EXISTS pg_eviltransform' >"$result_dir/create.log"
for run in 0 1 2 3 4 5; do
  ROWS=200000 scripts/benchmark_pg18.sh "$result_dir/coordinates-${run}.txt" >"$result_dir/coordinates-${run}.log" 2>&1
  ROWS=100000 DISTINCT_VALUES=1000 BREAKS=7 WORK_MEM=8MB scripts/benchmark_jenksbins.sh "$result_dir/jenks-${run}.txt" >"$result_dir/jenks-${run}.log" 2>&1
  echo "$phase PG$major run $run complete"
done
psql -At -v ON_ERROR_STOP=1 <<'SQL' >"$result_dir/results.txt"
SELECT md5(string_agg(encode(ST_AsEWKB(ST_EvilTransform(geom,990001)),'hex'),'' ORDER BY id)), md5(string_agg(encode(ST_AsEWKB(ST_EvilTransform(ST_EvilTransform(geom,990002),3857)),'hex'),'' ORDER BY id)) FROM bench_points;
SELECT ST_JenksBins(array_agg(value),7), ST_JenksBins(value,7) FROM bench_jenks_values;
SELECT ST_AsEWKT(ST_EvilTransform(g,990001)) FROM (VALUES ('SRID=4326;POINT ZM(120 30 5 7)'::geometry),('SRID=4326;LINESTRING(120 30,121 31)'::geometry),('SRID=4326;POLYGON((120 30,121 30,121 31,120 30))'::geometry),('SRID=4326;GEOMETRYCOLLECTION EMPTY'::geometry)) t(g);
SQL
