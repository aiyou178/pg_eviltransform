# `pg_eviltransform` Specification & Audit (v0.0.9, unreleased)

## 1. Purpose & Runtime Matrix

`pg_eviltransform` is a PostgreSQL and PostGIS extension built with Rust (`pgrx =0.20.0`, Rust `1.99.1`) and C (`librttopo` compiled with `-O3`). It provides:

1. **`ST_EvilTransform`**: In-database coordinate transformations between `WGS84` (`4326`), `CGCS2000` (`4490`), `GCJ-02` (`990001`), `BD-09` (`990002`), and any SRID or PROJ string supported by PostGIS `ST_Transform`.
2. **`ST_JenksBins`**: Exact Fisher-Jenks natural breaks classification (following PostGIS PR #6217 / `ST_JenksBins` semantics) as both array functions and streaming aggregates.
3. **`Regex_EvilTransform`** (`regex_eviltransform.sql`): Standalone SQL/PLpgSQL reference and benchmark baseline using regex WKT rewriting (referencing CN112000902B).

### Target Support Matrix

- **PostgreSQL**: `15.20+`, `16.16+`, `17.12+`, `18.7+`, `19.0+`
- **PostGIS**: `3.x` (`geometry` / `GSERIALIZED` storage format)
- **CI architectures**: `linux/amd64`, `linux/arm64`. macOS is not covered by this matrix.

---

## 2. Coordinate Reference System Registry

| System | Canonical SRID | Text Aliases (case-insensitive, trimmed) | Handling |
| :--- | :--- | :--- | :--- |
| **WGS 84** | `4326` | Handled by PostGIS (`EPSG:4326`, PROJ strings) | Standard PostGIS geographic CRS; intermediate pivot for cross-CRS transforms |
| **GCJ-02** | `990001` | `990001`, `EPSG:990001`, `GCJ02`, `GCJ-02` | Custom in-place `librttopo` point transform |
| **BD-09** | `990002` | `990002`, `EPSG:990002`, `BD09`, `BD-09` | Custom in-place `librttopo` point transform |
| **CGCS2000** | `4490` | `4490`, `EPSG:4490`, `CGCS2000`, `CGCS-2000` | Custom text alias resolved to SRID `4490`, delegated to PostGIS `ST_Transform` |

---

## 3. `ST_EvilTransform` Specification

### 3.1 Function Signatures

All overloads are `IMMUTABLE`, `STRICT`, and `PARALLEL SAFE`:

1. `st_eviltransform(geom anyelement, dst_srid integer) -> anyelement`
   - Rust/C fast path.
   - Accepts PostGIS `geometry` and domains over `geometry`, preserving the caller's type (or domain type) on return.
   - Validates that `geom.oid()` or `getBaseType(geom.oid())` equals the PostGIS `geometry` type OID before detoasting; raises `ERROR: ST_EvilTransform requires a PostGIS geometry` otherwise.
2. `st_eviltransform(geom geometry, to_proj text) -> geometry`
   - Rust routing resolves custom aliases or delegates to the schema-resolved PostGIS function OID.
3. `st_eviltransform(geom geometry, from_proj text, to_srid integer) -> geometry`
   - Rust routing overrides the input CRS with `from_proj`, then transforms to `to_srid`.
4. `st_eviltransform(geom geometry, from_proj text, to_proj text) -> geometry`
   - Rust routing overrides the input CRS with `from_proj`, then transforms to `to_proj`.

An explicit `from_proj` always takes precedence over `ST_SRID(geom)`, including
when the existing SRID is custom. Aliases relabel the input before routing;
standard PROJ strings are passed to PostGIS as the source CRS. NULL results from
PostGIS are preserved (for example, an unresolved source SRID with a text target).
Standard text targets follow PostGIS and return SRID 0. Alias parsing ignores case and
surrounding ASCII whitespace. The legacy SQL helper remains available as
`eviltransform_internal.__parse_custom_srid(text)` for existing dependencies.
Schema-independent lookup does not make PostGIS itself relocatable; its own
schema-relocation restrictions and SQL-body dependencies still apply.

### 3.2 Routing & Transformation Rules

Given input geometry with `src_srid = ST_SRID(geom)` and target `dst_srid`:

| `src_srid` | `dst_srid` | Execution Path | Output SRID |
| :--- | :--- | :--- | :--- |
| `s` | `s` (`src_srid == dst_srid`) | Identity fast return (no copy/reprojection) | `s` |
| `∉ {990001, 990002}` | `∉ {990001, 990002}` | Direct delegation to PostGIS `ST_Transform(geom, dst_srid)` | `dst_srid` |
| `4326` | `990001` | `librttopo` `MODE_WGS2GCJ` (`1`) | `990001` |
| `990001` | `4326` | `librttopo` `MODE_GCJ2WGS` (`2`) | `4326` |
| `4326` | `990002` | `librttopo` `MODE_WGS2BD` (`3`) | `990002` |
| `990002` | `4326` | `librttopo` `MODE_BD2WGS` (`4`) | `4326` |
| `990001` | `990002` | `librttopo` `MODE_GCJ2BD` (`5`) | `990002` |
| `990002` | `990001` | `librttopo` `MODE_BD2GCJ` (`6`) | `990001` |
| `4326` | `other` | PostGIS `ST_Transform(geom, dst_srid)` | `dst_srid` |
| `990001` | `other` | `MODE_GCJ2WGS` $\rightarrow$ `4326`, then `ST_Transform(wgs, dst_srid)` | `dst_srid` |
| `990002` | `other` | `MODE_BD2WGS` $\rightarrow$ `4326`, then `ST_Transform(wgs, dst_srid)` | `dst_srid` |
| `other` | `4326` | PostGIS `ST_Transform(geom, 4326)` | `4326` |
| `other` | `990001` | `ST_Transform(geom, 4326)`, then `MODE_WGS2GCJ` $\rightarrow$ `990001` | `990001` |
| `other` | `990002` | `ST_Transform(geom, 4326)`, then `MODE_WGS2BD` $\rightarrow$ `990002` | `990002` |

### 3.3 Geometry & Mathematical Semantics

- **Dimensionality & Flags**: Transforms X/Y while keeping Z/M unchanged. Drops stale cached bounding boxes and recomputes them with `rtgeom_drop_bbox` / `rtgeom_add_bbox` before serialization, preserving the `GSERIALIZED` geodetic flag. Empty geometries retain their shape with the target SRID.
- **China Bounding Box Check (`out_of_china`)**:
  - `lng < 72.004 || lng > 137.8347 || lat < 0.8293 || lat > 55.8271`
  - Outside this bounding box, all four elementary native Rust/C steps (`WGS84 <-> GCJ-02` and `GCJ-02 <-> BD-09`) leave `(lng, lat)` unchanged. Chained steps each apply their own guard; output SRID still changes.
  - The standalone regex SQL reference differs: its GCJ-02 / BD-09 polar offset is unconditional. It is a historical baseline, not a bit-identical oracle for native results.
- **Ellipsoid & Constants**:
  - `EARTH_R = 6378137.0`
  - `EE = 0.00669342162296594323`
  - `X_PI = PI * 3000.0 / 180.0`
  - `GCJ2WGS` uses single-step first-order delta subtraction: `wgs = 2 * gcj - wgs2gcj(gcj)`.

---

## 4. `ST_JenksBins` Specification

### 4.1 Function Signatures

- **Array Overloads (`IMMUTABLE STRICT PARALLEL SAFE`; nullable result)**:
  - `st_jenksbins(values T[], breaks integer) -> double precision[]`
  - `st_jenksbins(values T[], breaks integer, invert boolean) -> double precision[]`
  - Supported element types `T`: `double precision`, `real`, `bigint`, `integer`, `smallint`, `numeric`.
- **Aggregate Overloads (`STYPE = internal`)**:
  - `st_jenksbins(value T, breaks integer) -> double precision[]`
  - `st_jenksbins(value T, breaks integer, invert boolean) -> double precision[]`
  - Supported element types `T`: `double precision`, `numeric`.

### 4.2 Classification Semantics

- **NULL & Empty Handling**:
  - SQL `NULL` array inputs, empty arrays, arrays containing only `NULL` elements, or aggregate groups with zero non-`NULL` rows return `NULL`.
  - Non-`NULL` elements are converted to `f64`. `AnyNumeric` uses pgrx's native PostgreSQL `numeric_float8` conversion, not a string round-trip.
- **Validation**:
  - `breaks < 1` raises `ERROR: breaks must be greater than or equal to 1` for evaluated calls (STRICT functions skip NULL arguments).
  - `NaN` or infinity raises `ERROR: Jenks input must be finite f64, got ...`; numeric overflow raises a conversion error.
  - Aggregate calls where `breaks` or `invert` vary across rows in the same group raise `ERROR: ST_JenksBins aggregate breaks and invert arguments must be constant`.
- **Output Contract**:
  - Let $m$ be the count of distinct finite values (after normalizing `-0.0` to `+0.0`) and $k = \text{breaks}$.
  - If $m \le k$, returns all $m$ sorted distinct values.
  - If $m > k$, partitions the sorted distinct values into $k$ contiguous non-empty classes minimizing total within-class sum of squared deviations (SSD):
    - When `invert = false` (default): returns the **maximum** value of each class (length $k$, ending with the global maximum).
    - When `invert = true`: returns the **minimum** value of each class (length $k$, starting with the global minimum).
  - Unweighted inputs (all frequencies equal 1) use `natural_breaks::classify_indices`; weighted inputs use divide-and-conquer Monge dynamic programming in $O(k \cdot m \log m)$ time.

---

## 5. Audit Resolution

The original audit described v0.0.8. The following reconciles its claims with
the source and the v0.0.9 changes. Released migration files are unchanged.

| Finding | Resolution |
| --- | --- |
| 5.1: text overloads depend on caller search_path | Fixed. All three retain their geometry signatures but now use Rust routing and PostGIS OIDs resolved in the extension's owning schema. |
| 5.2: process-lifetime PostGIS OIDs become stale | Fixed. Cache entries are database-keyed and invalidated by procedure, type and namespace syscache callbacks, including the object changes made by extension recreation. Callbacks only increment a generation; lookups occur outside callbacks and cache borrows. Schema renames and same-session extension recreation are regression-covered. |
| 5.3: missing 0.0.1-0.0.6 migration chain | Incorrect. The repository already contains 0.0.2 through 0.0.8 migrations; its initial source version is 0.0.2. No released 0.0.1 schema is established, so no speculative migration is added. |
| 5.4: explicit source CRS overrides input SRID | Intended PostGIS-compatible behavior, now documented and tested even for conflicting input SRIDs. |
| 5.5: differing coordinate mathematics | Native Earth radius 6378137.0 and regex SQL radius 6378245.0 are intentional historical differences. Native elementary steps already all have China guards. No formulas or boundaries changed. |
| 5.5: regex does not accept signs, integers or exponents | Stale: the existing token regex already accepts these. A separate real defect was found and fixed: preserve Z/M in both POINT and multi-point WKT rewriting. |
| Additional: borrowed input mutation | Fixed. librttopo's writable iterator aliases serialized coordinate storage. Custom transforms now obtain an owned writable copy when detoasting returned a borrowed datum; repeated use of an in-memory geometry must not compound offsets. |
| Additional: stale cached geometry bounds | Fixed. On published 0.0.8, a transformed boxed LINESTRING can fail both geometry && its_own_endpoint and the ND &&& equivalent. Envelope/Box2D checks alone missed this because they can recompute bounds. Refresh deserialized boxes before serialization; regression checks exercise the actual cached-box predicates. |

### Upgrade

Install the 0.0.9 shared library and SQL files, then run:

```sql
ALTER EXTENSION pg_eviltransform UPDATE TO '0.0.9';
```

The new 0.0.8--0.0.9 migration replaces text-overload bodies in place, preserving
their function OIDs, argument names and dependent objects. It discovers both
extension schemas from PostgreSQL catalogs rather than assuming public.

Recompute stored custom-transform results from the original source geometries,
refresh affected materialized views, and rebuild expression indexes using these transforms. Library replacement does
not repair old serialized bounding boxes or index entries.

## 6. Performance Decisions

| Original opportunity | Decision |
| --- | --- |
| 6.1: repeated detoasting | Implemented: integer routing retains one detoasted input through SRID inspection and custom transformation, freeing only owned copies. Borrowed uncompressed input is copied before mutation; already-owned detoast buffers are reused without decompression again. Identity results remain valid after cleanup. |
| 6.2: PostgreSQL allocators / direct GSERIALIZED mutation | Deferred. Keep librttopo's format-aware deserialization and malloc ownership, with a Rust RAII guard for its output buffer. Replacing allocators introduces PostgreSQL ERROR/unwind concerns; hand-editing storage risks version, box, curve and collection compatibility. Correct cached-box recomputation takes precedence over skipping that work for speed. |
| 6.2: generic point iterator overhead | Profiled and optimized: simple Cartesian POINT/LINESTRING use librttopo's indexed point-array APIs without iterator stacks. Curves, collections and geodetic storage retain the generic iterator. Ownership guards and bounding-box refresh are unchanged. |
| 6.3: cache initialized FmgrInfo | Deferred pending workload-specific evidence and an explicit invalidation-safe memory-context design. OID caching is corrected; per-call fmgr initialization remains. A cached SQL PostGIS function also owns fn_extra state, so this is not just copying a function pointer. |
| 6.4: two-stage intermediate geometries | Implemented: release owned intermediate varlenas after the final stage, with input/output alias checks. |
| 6.5: hypot, sincos and constant rearrangements | Deferred: no demonstrated end-to-end gain warrants changing rounding or portable libm behavior. Constant expressions are compiler-foldable; claimed 3-5x speedups are unverified. |
| 6.6: eager array Vec | Implemented: all six element types and both array overloads consume pgrx Array iterators without a full intermediate Vec<Option<T>>. |
| 6.6: numeric string round-trip | Already optimized in v0.0.8: f64::try_from(AnyNumeric) calls numeric_float8 in pgrx 0.20.0. |
| 6.6: Jenks DP allocations | Implemented: one flat checked-size split matrix and direct edge indexing, removing the final distinct-value copy. |
| 6.6: alternate hashing / sorting | Deferred pending distinct-count and memory measurements. Preserve duplicate-heavy streaming behavior and standard randomized hashing rather than adding a dependency or a second algorithm. |
| 6.6: parallel aggregation | Deferred as a separate feature. Requires combine/serialization contracts, cross-worker argument validation and error/lifecycle tests, not merely PARALLEL SAFE. Existing aggregate signatures are unchanged. |

Builds retain generic CPU targets, Rust opt-level=3/fat LTO and C -O3. No
native-CPU, fast-math, unsafe-postgres or installation-privilege changes are made.

## 7. Regression Coverage

- pgrx tests: POINT/LINESTRING in 2D/Z/M/ZM, collection fallback, transformed
  bounding-box containment, Z/M preservation, explicit
  source override, empty geometries, toasted inputs and identity result lifetime;
  array tests cover all element types, NULL/empty input and inverted edges.
- `tests/catalog_lifecycle.sql`: separate extension schemas, restricted caller
  search_path, serialized 2D/ND box predicates, schema rename, same-backend
  PostGIS recreation, and regex Z/M.
  Run it in a disposable database; CI runs it on PostgreSQL 15-19 / amd64-arm64.
- Existing core tests retain coordinate reference cases and known Jenks clusters.
  Weighted tests now also exercise lower edges, one class and multiple splits.

Raw benchmark output and machine-specific intermediate files remain local.
The [final benchmark report](benchmarks/2026-11-22-spec-audit.md) records the
initial correctness-related costs and subsequent profile-guided improvements.

Initial verification on 2026-11-22 passed on Linux ARM64: 32 tests per PostgreSQL
major (15.20, 16.16, 17.12, 18.7, 19.0), the full catalog/cached-box fixture on
all five versions, 13 standalone core tests, and all five DEB builds. Package
inspection verified architecture, version and migration contents. A real
published-0.0.8-to-0.0.9 upgrade preserved function OIDs and a dependent view.
Native amd64/arm64 coverage is provided by the PostgreSQL 15-19 CI matrix;
the local measurements above describe ARM64 only. Consult the PR checks for
the final commit's CI result.

The subsequent traversal optimization reran all 32 tests and the catalog/cached-box
fixture on each of the five PostgreSQL versions. An additional 358-case matrix
preserved EWKB results, input ownership and applicable cached-box predicates.
Two independent 20-pair benchmark batches showed 6.6-7.4% lower composed-query
medians and 11.8-12.3% lower toasted-line medians versus the correct baseline.
