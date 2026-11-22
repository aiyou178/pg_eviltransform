# Spec audit benchmark: 0.0.8 vs 0.0.9

Local Linux ARM64 comparison on PostgreSQL 19.0 / PostGIS 3.7.0, using the
published 0.0.8 binary and the corrected, unreleased 0.0.9 release-profile build. Both use pgrx
0.20.0, Rust 1.99.1, generic CPU targets, fat LTO and C -O3.

## Method

- One excluded warm-up and five measured runs per workload, alternating version
  order. Benchmark containers used the same CPU allocation, separate from build
  workloads. JIT and query parallelism were disabled; work_mem was 8 MB.
- Points: 200,000 WGS84 points, uniform within the China bounding box, seed 0.42.
- Toasted lines: 256 externally stored LineStrings, 1,000 vertices per geometry.
- Jenks: 100,000 float8 values, 1,000 distinct values, seven classes.
- Standard text target: the first 1,000 points, transformed to
  `+proj=longlat +datum=WGS84 +no_defs`.
- Timings are EXPLAIN ANALYZE execution times with TIMING OFF. Coordinate queries
  consume only ST_X (ST_StartPoint first for lines), evaluating each transformed
  geometry once. These are not directly comparable with the older X-plus-Y
  benchmark tables, which may evaluate the transform twice.

## Correctness-fix baseline

This first comparison predates the profile-guided traversal optimization below.

Times are milliseconds. Positive change means slower; ranges are measured min-max.

| Workload | 0.0.8 median (range) | 0.0.9 median (range) | Change |
| --- | ---: | ---: | ---: |
| Native 4326 to GCJ02 | 70.310 (69.419-73.274) | 68.876 (68.522-73.044) | -2.0% |
| Native 4326 to BD09 to 3857 | 180.967 (178.367-210.072) | 188.849 (185.553-200.218) | +4.4% |
| Toasted lines, 4326 to GCJ02 | 23.270 (22.175-26.550) | 24.532 (23.489-26.685) | +5.4% |
| Jenks array | 8.170 (7.690-8.633) | 7.686 (7.438-7.768) | -5.9% |
| Jenks aggregate | 9.088 (8.860-9.166) | 9.078 (8.714-9.219) | -0.1% |
| Standard text target, 1,000 points | 60.131 (59.102-61.466) | 52.503 (50.211-52.741) | -12.7% |

All numeric coordinate-query outputs and Jenks edges matched between versions.
This is not a claim of identical serialized geometry: 0.0.9 repairs cached boxes
that could cause false negatives in spatial filters.

Text routing and borrowed Jenks arrays improved these samples. Direct coordinate
and aggregate timings overlap, so there is no clear general improvement. Composed
queries and lines have higher medians: preserving borrowed inputs and refreshing
cached spatial bounds take precedence over avoiding those allocations/scans.
These local samples do not establish performance on amd64 or other workloads.

The [specification](../spec.md) records implemented fixes and deferred
optimizations. Raw plans, drivers and intermediate runs remain outside the repo.

## Profile-guided optimization (latest)

The baseline for this follow-up is the **correct** pre-optimization 0.0.9 build,
not the unsafe 0.0.8 implementation. Linux perf sampled each workload at 199 Hz
for 20 seconds before and after the change. Leaf CPU samples showed:

- Line iterator routines accounted for approximately 7.3% before the change and
  had no samples afterward in this simple-LineString workload.
- malloc/free accounted for approximately 4.3% of composed-query samples before
  the change and 3.1% afterward. These are CPU shares, not allocation counts.
- Bounding-box calculation remains present. Trigonometry remains substantial;
  no formulas, precision settings or CPU-specific optimizations were changed.

Simple Cartesian POINT and LINESTRING geometries now traverse their point arrays
with librttopo's indexed getters/setters, avoiding the generic iterator's stack
allocations and per-vertex navigation. Curves, collections and geodetic storage
retain the generic iterator. Input-copy ownership guards, Z/M preservation and
cached bounding-box recomputation remain intact.

### Paired timing

Both builds used the same toolchain, flags, PostgreSQL process environment and
data in one container. Separate C entry points selected the frozen correct
baseline and optimized library, with identical SQL attributes and query plans.
Profiling was disabled during timings. Each batch excluded three warm-ups, then
ran 20 pairs with deterministic randomized version order. CPU affinity was fixed;
unrelated local activity can still contribute noise.

The repeat batch's medians are below; negative change means faster.

| Workload | Correct baseline | Optimized | Change |
| --- | ---: | ---: | ---: |
| Composed 4326 to BD09 to 3857 | 202.799 ms | 187.831 ms | -7.4% |
| Toasted lines, 4326 to GCJ02 | 26.091 ms | 22.883 ms | -12.3% |
| Direct points, 4326 to GCJ02 (control) | 71.177 ms | 63.575 ms | -10.7% |

The first independent 20-pair batch showed 6.6% lower composed-query medians and
11.8% lower line-query medians. All timed-query results matched exactly. Do not
compare these absolute times with the earlier table or combine their percentages;
the follow-up uses paired measurements in a different run environment.

### Correctness checks

- All 32 release tests and the catalog/cached-box fixture passed on each of
  PostgreSQL 15.20, 16.16, 17.12, 18.7 and 19.0 on Linux ARM64.
- The native regression covers POINT/LINESTRING in 2D, Z, M and ZM forms, empty
  geometries and collection fallback, including cached 2D/ND spatial predicates.
- A separate 358-case matrix matched the correct baseline's EWKB byte-for-byte
  and checked input immutability and applicable cached-box predicates. It includes
  custom and standard SRID routing, curves, polygons, collections, TINs, NaN,
  infinity and signed zero.

These are local ARM64 findings, not a performance guarantee on amd64. Raw perf
data, source snapshots, query plans and timing drivers remain outside the repo.
