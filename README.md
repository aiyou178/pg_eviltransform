<p align="center">
  <img src="assets/logo/pg-eviltransform-512.png" alt="pg_eviltransform logo" width="160" height="160">
</p>

# pg_eviltransform

[中文](README.zh-CN.md) | [Logo assets](assets/logo)

PostGIS coordinate conversion for GCJ02, BD09 and CGCS2000, plus exact Jenks natural breaks.

## Installation

Requires PostGIS and PostgreSQL with the following minimum versions for both
the server runtime and build headers. Later minor releases within each listed
major version are supported:

| PostgreSQL | Minimum runtime / build headers |
| --- | --- |
| 15 | 15.20+ |
| 16 | 16.16+ |
| 17 | 17.12+ |
| 18 | 18.7+ |
| 19 | 19rc2, experimental |

These are compatibility requirements, not just CI test versions. Current
builds use the patched PostgreSQL headers required for pgrx 0.20.0's composite
datum security fix (CVE-2026-6460); binaries built against those headers cannot
load on earlier minor releases. PG19 support is experimental and targets 19rc2.
PG14 is no longer supported by current source builds. Binary availability is
release-specific: the [pgext.cloud catalog](https://pgext.cloud/ext/pg_eviltransform)
currently lists older 0.0.7 / pgrx 0.19.3 packages for PG14-18. Check its
PostgreSQL, OS and architecture matrix before choosing a package.

### Install server packages

With [pig](https://pig.pgsty.com) installed, enable the catalog's repository
and install for your PostgreSQL major (PG18 shown):

```bash
pig repo add pgsql -u
pig install pg_eviltransform -v 18
```

With the same repository configured, you can instead use:

```bash
sudo apt install postgresql-18-eviltransform  # Debian / Ubuntu
sudo dnf install pg_eviltransform_18          # RPM distributions
```

Alternatively, use a matching binary from [GitHub Releases](https://github.com/aiyou178/pg_eviltransform/releases),
or [build from source](#development). Upstream DEB filenames use
`postgresql-18-pg-eviltransform_*.deb`; catalog package names differ.

### Enable in a database

Run as a database administrator after installing the server packages:

```sql
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION pg_eviltransform;
SELECT extversion FROM pg_extension WHERE extname = 'pg_eviltransform';
```

No `shared_preload_libraries` setting is needed. Application roles can call the
installed API; text overloads also need `USAGE` on `eviltransform_internal`.

### Upgrade from 0.0.7

Upgrade PostgreSQL to a supported minor version and install the matching 0.0.8
package. Reconnect to the database, then run:

```sql
ALTER EXTENSION pg_eviltransform UPDATE TO '0.0.8';
```

## Usage

### Coordinate conversion

`ST_EvilTransform` follows the four `ST_Transform` overloads:

```text
ST_EvilTransform(geometry, to_srid integer)
ST_EvilTransform(geometry, to_proj text)
ST_EvilTransform(geometry, from_proj text, to_srid integer)
ST_EvilTransform(geometry, from_proj text, to_proj text)
```

| Coordinate system | SRID | Text aliases |
| --- | --- | --- |
| WGS84 | 4326 | `EPSG:4326` |
| GCJ02 | 990001 | `GCJ02`, `GCJ-02`, `EPSG:990001` |
| BD09 | 990002 | `BD09`, `BD-09`, `EPSG:990002` |
| Geographic CGCS2000 | 4490 | `CGCS2000`, `CGCS-2000`, `EPSG:4490` |

```sql
-- WGS84 to GCJ02 or BD09
SELECT ST_EvilTransform('SRID=4326;POINT(120 30)'::geometry, 'GCJ02');
SELECT ST_EvilTransform('SRID=4326;POINT(120 30)'::geometry, 990002);

-- BD09 to Web Mercator
SELECT ST_EvilTransform('SRID=990002;POINT(120.01107 30.00388)'::geometry, 3857);

-- Explicit source and target systems
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 'GCJ02');
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'CGCS2000', 990001);
```

Standard systems delegate to PostGIS/PROJ; custom systems bridge through WGS84
when needed. CGCS2000 aliases mean geographic EPSG:4490; projected Gauss-Kruger
zones require their own explicit EPSG codes.

### Jenks natural breaks

`ST_JenksBins` returns `double precision[]` class edges:

```sql
SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::double precision[], 2);
-- {2,11}

SELECT ST_JenksBins(value::double precision, 7) FROM measurements;
SELECT ST_JenksBins(value::double precision, 7, true) FROM measurements;
-- true selects lower edges; the default is upper edges
```

Arrays accept `numeric`, `double precision`, `real`, `bigint`, `integer` and
`smallint`; aggregates accept `numeric` and `double precision`. NULLs are ignored,
empty input returns NULL, and non-finite values or fewer than one class raise
an error. If distinct values do not exceed the class count, sorted unique
values are returned. Numeric inputs are computed as finite f64 values.

## Benchmark

Local ARM64, pgrx 0.20.0, PostGIS 3.7.0, JIT/parallel query off;
median of five runs after one warm-up. Coordinate tests use 200,000 points.

| PostgreSQL | Conversion | Native | Regex SQL |
| --- | --- | ---: | ---: |
| 18.7 | 4326 -> GCJ02 | 120.61 ms | 5546.00 ms |
| 18.7 | 4326 -> BD09 -> 3857 | 320.75 ms | 15343.10 ms |
| 19rc2 | 4326 -> GCJ02 | 117.47 ms | 5457.14 ms |
| 19rc2 | 4326 -> BD09 -> 3857 | 321.29 ms | 14996.98 ms |

Jenks: 100,000 rows, 1,000 distinct values, 7 classes, work_mem=8MB:

| PostgreSQL | Rust array | Rust aggregate | CartoDB SQL |
| --- | ---: | ---: | ---: |
| 18.7 | 10.49 ms | 9.11 ms | 84.07 ms |
| 19rc2 | 9.25 ms | 8.36 ms | 82.90 ms |

The [full report](doc/benchmarks/2026-11-13-pgrx-0.20.0.md) includes before/after
comparisons, timing ranges and differences between the SQL baselines
and native functions. Results depend on the workload and host.

Reproduce in a disposable database (the scripts recreate benchmark tables):

```bash
PGDATABASE=bench ROWS=200000 PG_MAJOR=18 scripts/benchmark_pg18.sh /tmp/coordinates.txt
PGDATABASE=bench ROWS=100000 DISTINCT_VALUES=1000 BREAKS=7 WORK_MEM=8MB \
  scripts/benchmark_jenksbins.sh /tmp/jenks.txt
```

## Development

Use Rust **1.99.1**, cargo-pgrx **0.20.0**, matching Clang/libclang/LLVM majors,
PostgreSQL server headers, PostGIS, and ICU/GEOS/librttopo development libraries.
See the [benchmark Dockerfile](docker/Dockerfile.benchmark-trixie) for Debian dependencies.

```bash
rustup toolchain install 1.99.1 --profile minimal
cargo install --locked cargo-pgrx --version 0.20.0
cargo pgrx init --pg18=/usr/lib/postgresql/18/bin/pg_config
cargo test --locked --lib
cargo pgrx package --release --no-default-features --features pg18 \
  --pg-config /usr/lib/postgresql/18/bin/pg_config
cargo pgrx test pg18 --release --no-default-features --features pg_test
```

Replace `pg18` and the header path for other majors. Distro PostgreSQL tests
need permission to install extension files; root/container runs should add
`--runas postgres --pgdata /tmp/pg-eviltransform-tests` to the test command.

The CI matrix builds, tests and benchmarks **PG15-19 on amd64 and arm64**.
Release builds use portable CPU targets, `opt-level=3`, fat LTO and C `-O3`.

```bash
scripts/release_debian_trixie.sh  # Docker: PG15-19 DEBs in dist/
```

The tag-triggered release workflow also produces RPMs.

## Acknowledgments and License

- [googollee/eviltransform](https://github.com/googollee/eviltransform): coordinate formulas.
- [PostGIS](https://postgis.net/) and [librttopo](https://git.osgeo.org/gitea/rttopo/librttopo): geometry processing.
- [pgrx](https://github.com/pgcentralfoundation/pgrx) and [natural-breaks](https://github.com/uratne/natural-breaks): Rust extension framework and Jenks implementation.
- [CartoDB CDB_JenksBins](https://github.com/CartoDB/cartodb-postgresql/blob/master/scripts-available/CDB_JenksBins.sql): benchmark-only SQL baseline, copyright 2014 Vizzuality, BSD 3-Clause.

### Citation

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

### License

Project source is [MIT licensed](LICENSE). librttopo is GPL-2.0-or-later;
third-party components retain their own licenses.
