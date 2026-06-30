# pg_eviltransform (Rust/pgrx)

[中文文档 (Chinese README)](README.zh-CN.md)

`pg_eviltransform` extends PostGIS `ST_Transform` with BD09/GCJ02 support.

It exposes one public function name, following the same overload interface as `ST_Transform`:

- `ST_EvilTransform(geometry, to_srid integer)`
- `ST_EvilTransform(geometry, to_proj text)`
- `ST_EvilTransform(geometry, from_proj text, to_srid integer)`
- `ST_EvilTransform(geometry, from_proj text, to_proj text)`

Behavior:

- If neither side uses custom coordinates, it delegates directly to `ST_Transform`.
- If BD09/GCJ02 is involved, it transforms via WGS84 (`4326`) when needed.

Custom SRIDs:

- `990001`: GCJ02
- `990002`: BD09

## Regex SQL Variant

`regex_eviltransform.sql` defines `Regex_EvilTransform(...)` with the same overload interface as `ST_EvilTransform(...)`.

`Regex_EvilTransform` is a SQL/PLpgSQL + regex implementation used for comparison and benchmarking against `pg_eviltransform`.

## Citation

Patent reference for the regex-based SQL approach:

- [CN112000902B](https://patents.google.com/patent/CN112000902B/zh)

If you use the regex-based SQL approach, cite:

```bibtex
@patent{CN112000902B,
  title     = {用于地图区域绘制的方法、电子设备和存储介质},
  author    = {梁展钊},
  number    = {CN112000902B},
  year      = {2021},
  type      = {Chinese Patent},
  assignee  = {Shanghai Maice Data Technology Co., Ltd.; Maice (Shanghai) Intelligent Technology Co., Ltd.},
  url       = {https://patents.google.com/patent/CN112000902B}
}
```

## Thanks

- [googollee/eviltransform](https://github.com/googollee/eviltransform) for the open-source coordinate transform implementation and reference formulas.

## Requirements

- Rust stable 1.96+
- `cargo-pgrx` 0.19.1:
  `cargo install --locked cargo-pgrx --version 0.19.1`
- PostgreSQL + PostGIS (PG 14-19 packages; PG19 beta is experimental until PostgreSQL 19 GA)
- `librttopo-dev` (for the native GSERIALIZED fast path)

Packaging note:

- DEB packages have no hard runtime package dependency to keep installation lightweight.
- PostgreSQL/PostGIS are kept as suggested packages to avoid pulling a large dependency tree during `apt install`.
- Runtime still requires PostGIS (`CREATE EXTENSION postgis`) before `CREATE EXTENSION pg_eviltransform`.

## Build

```bash
cargo pgrx init \
  --pg14=/usr/lib/postgresql/14/bin/pg_config \
  --pg15=/usr/lib/postgresql/15/bin/pg_config \
  --pg16=/usr/lib/postgresql/16/bin/pg_config \
  --pg17=/usr/lib/postgresql/17/bin/pg_config \
  --pg18=/usr/lib/postgresql/18/bin/pg_config \
  --pg19=/usr/lib/postgresql/19/bin/pg_config

cargo pgrx package --release --features pg18 --no-default-features --pg-config /usr/lib/postgresql/18/bin/pg_config
# Experimental PG19 beta:
cargo pgrx package --release --features pg19 --no-default-features --pg-config /usr/lib/postgresql/19/bin/pg_config
```

## Test (native Rust)

```bash
cargo test
cargo pgrx test pg18 --features "pg18 pg_test"
# Experimental PG19 beta:
cargo pgrx test pg19 --features "pg19 pg_test"
```

## Usage

```sql
-- Preferred: explicit literals (no need to remember custom SRID numbers)
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 'GCJ02');
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 'BD09');

-- Equivalent numeric custom SRID form
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 990001);

-- BD09 (990002) -> Web Mercator (3857)
SELECT ST_EvilTransform(ST_SetSRID('POINT(120.011070620552 30.0038830555128)'::geometry, 990002), 3857);

-- from_proj / to_proj overload with literals
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 'GCJ02');
```

## Jenks Natural Breaks

`ST_JenksBins` computes exact Jenks natural breaks and returns `double precision[]` bin edges.

Supported array inputs:

- `numeric[]`
- `double precision[]`
- `real[]`
- `bigint[]`
- `integer[]`
- `smallint[]`

Supported aggregate inputs:

- `numeric`
- `double precision`

Integer and `real` aggregate inputs can be cast to `double precision` or `numeric`. `numeric` inputs are accepted, but computation is normalized to finite `f64`, so output edges are floating-point values.

Examples:

```sql
-- Array form. NULL elements are ignored.
SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::numeric[], 2);

-- Streaming aggregate form. Prefer this for large tables.
SELECT ST_JenksBins(value, 7)
FROM big_table;

-- Lower bin edges instead of the default upper edges.
SELECT ST_JenksBins(value, 7, true)
FROM big_table;
```

Behavior:

- `NULL` inputs are ignored.
- `breaks < 1`, `NaN`, infinity, and numeric values outside finite `f64` conversion range error.
- No valid input rows return `NULL`.
- If the distinct value count is less than or equal to `breaks`, sorted unique values are returned.

## Benchmark (PG18)

Use the benchmark script to compare `ST_EvilTransform` and `Regex_EvilTransform`:

```bash
# optional env: PGHOST, PGPORT, PGUSER, PGDATABASE, ROWS
scripts/benchmark_pg18.sh
# Experimental PG19 beta:
PG_MAJOR=19 scripts/benchmark_pg18.sh
```

It will:

- Ensure `postgis` and `pg_eviltransform` extensions exist.
- Load `regex_eviltransform.sql` (creating `Regex_EvilTransform`).
- Generate benchmark data in `public.bench_points`.
- Run `EXPLAIN (ANALYZE, BUFFERS)` for `ST_EvilTransform` and `Regex_EvilTransform`.
- Write report to `benchmark_pg18_report.txt` (or custom output path).

Example:

```bash
ROWS=500000 PGDATABASE=testdb scripts/benchmark_pg18.sh /tmp/bench_pg18.txt
```

Latest run (PG18, `ROWS=200000`, report: `benchmark_pg18_report.txt`):

| Scenario | `ST_EvilTransform` | `Regex_EvilTransform` | Speedup (`Regex` / `ST`) |
|---|---:|---:|---:|
| `4326 -> 990001` | `92.402 ms` | `2832.821 ms` | `30.7x` |
| `990002 -> 3857 (via 4326)` | `183.856 ms` | `8393.272 ms` | `45.7x` |

Experimental PG19 beta run (`postgres:19beta1-trixie`, `ROWS=200000`, report: `benchmark_pg19_report.txt`):

| Scenario | `ST_EvilTransform` | `Regex_EvilTransform` | Speedup (`Regex` / `ST`) |
|---|---:|---:|---:|
| `4326 -> 990001` | `100.307 ms` | `2874.719 ms` | `28.7x` |
| `990002 -> 3857 (via 4326)` | `182.430 ms` | `8230.002 ms` | `45.1x` |

## Jenks Benchmark

Use `scripts/benchmark_jenksbins.sh` to compare CartoDB's SQL baseline with Rust array and streaming aggregate forms:

```bash
# optional env: ROWS, DISTINCT_VALUES, BREAKS, WORK_MEM, PGHOST, PGPORT, PGUSER, PGDATABASE
scripts/benchmark_jenksbins.sh
```

The CartoDB baseline SQL used by the benchmark is vendored at `scripts/CDB_JenksBins.sql` with upstream attribution and license notes.

PG19 beta Docker run (`ROWS=100000`, `DISTINCT_VALUES=1000`, `BREAKS=7`, `WORK_MEM=8MB`):

| Scenario | Execution time |
|---|---:|
| `CDB_JenksBins(array_agg(value::numeric), breaks)` | `95.785 ms` |
| `ST_JenksBins(array_agg(value), breaks)` | `11.381 ms` |
| `ST_JenksBins(value, breaks)` streaming aggregate | `8.210 ms` |

The streaming aggregate avoids materializing an `array_agg` input and keeps an internal distinct-value count map during aggregation.

## Release Debian (Trixie, PG14-19)

```bash
scripts/release_debian_trixie.sh
```

This builds `.deb` artifacts under `dist/`:

- `postgresql-14-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-15-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-16-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-17-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-18-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-19-pg-eviltransform_<version>_trixie_<arch>.deb`

GitHub Release CI publishes both `amd64` and `arm64` package artifacts (`.deb` + `.rpm`).

## License

MIT. See `LICENSE`.
