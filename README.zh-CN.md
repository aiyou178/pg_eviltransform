# pg_eviltransform (Rust/pgrx)

[English README](README.md)

`pg_eviltransform` 是对 PostGIS `ST_Transform` 的扩展，增加了 BD09 / GCJ02 坐标系支持。

对外只提供一个函数名，接口与 `ST_Transform` 保持一致：

- `ST_EvilTransform(geometry, to_srid integer)`
- `ST_EvilTransform(geometry, to_proj text)`
- `ST_EvilTransform(geometry, from_proj text, to_srid integer)`
- `ST_EvilTransform(geometry, from_proj text, to_proj text)`

行为说明：

- 如果源/目标都不是自定义坐标系，直接委托给 `ST_Transform`。
- 如果涉及 BD09/GCJ02，会在需要时通过 WGS84（`4326`）进行桥接转换。

自定义 SRID：

- `990001`: GCJ02
- `990002`: BD09

## Regex SQL 对照实现

`regex_eviltransform.sql` 提供 `Regex_EvilTransform(...)`，重载接口与 `ST_EvilTransform(...)` 一致。

`Regex_EvilTransform` 基于 SQL/PLpgSQL + 正则，主要用于与 `pg_eviltransform` 做性能对比。

## 引用

Regex SQL 方案参考专利：

- [CN112000902B](https://patents.google.com/patent/CN112000902B/zh)

如使用基于 regex 的 SQL 实现，请引用：

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

## 致谢

- 感谢 [googollee/eviltransform](https://github.com/googollee/eviltransform) 提供的开源坐标转换实现与参考公式。

## 依赖要求

- Rust stable 1.96+
- `cargo-pgrx` 0.19.1：
  `cargo install --locked cargo-pgrx --version 0.19.1`
- PostgreSQL + PostGIS（发布 PG 14-19 包；PG19 beta 在 PostgreSQL 19 GA 前仍为实验支持）
- `librttopo-dev`（用于原生 GSERIALIZED 快速路径）

打包说明：

- DEB 包不设置硬依赖，保持安装轻量。
- PostgreSQL/PostGIS 作为建议依赖（`Suggests`），避免 `apt install` 时自动拉取大量 GIS 依赖链。
- 运行时仍需要先安装并创建 PostGIS（`CREATE EXTENSION postgis`），再创建 `pg_eviltransform`。

## 构建

```bash
cargo pgrx init \
  --pg14=/usr/lib/postgresql/14/bin/pg_config \
  --pg15=/usr/lib/postgresql/15/bin/pg_config \
  --pg16=/usr/lib/postgresql/16/bin/pg_config \
  --pg17=/usr/lib/postgresql/17/bin/pg_config \
  --pg18=/usr/lib/postgresql/18/bin/pg_config \
  --pg19=/usr/lib/postgresql/19/bin/pg_config

cargo pgrx package --release --features pg18 --no-default-features --pg-config /usr/lib/postgresql/18/bin/pg_config
# 实验性 PG19 beta：
cargo pgrx package --release --features pg19 --no-default-features --pg-config /usr/lib/postgresql/19/bin/pg_config
```

## 测试（Rust 原生）

```bash
cargo test
cargo pgrx test pg18 --features "pg18 pg_test"
# 实验性 PG19 beta：
cargo pgrx test pg19 --features "pg19 pg_test"
```

## 使用示例

```sql
-- 推荐：直接使用字面量（不需要记忆自定义 SRID 数字）
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 'GCJ02');
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 'BD09');

-- 等价的数字 SRID 写法
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 990001);

-- BD09 (990002) -> Web Mercator (3857)
SELECT ST_EvilTransform(ST_SetSRID('POINT(120.011070620552 30.0038830555128)'::geometry, 990002), 3857);

-- from_proj / to_proj 重载 + 字面量
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 'GCJ02');
```

## Jenks 自然断点

`ST_JenksBins` 计算精确 Jenks natural breaks，并返回 `double precision[]` 分箱边界。

支持的数组输入：

- `numeric[]`
- `double precision[]`
- `real[]`
- `bigint[]`
- `integer[]`
- `smallint[]`

支持的聚合输入：

- `numeric`
- `double precision`

整数和 `real` 聚合输入可显式转换为 `double precision` 或 `numeric`。`numeric` 可以作为输入，但内部会归一化为有限 `f64` 参与计算，因此返回边界是浮点值。

示例：

```sql
-- 数组形式。NULL 元素会被忽略。
SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::numeric[], 2);

-- 流式聚合形式。大表优先使用这种方式。
SELECT ST_JenksBins(value, 7)
FROM big_table;

-- 返回下边界，而不是默认的上边界。
SELECT ST_JenksBins(value, 7, true)
FROM big_table;
```

行为：

- 忽略 `NULL` 输入。
- `breaks < 1`、`NaN`、无穷值，以及不能转换为有限 `f64` 的 `numeric` 会报错。
- 没有有效输入行时返回 `NULL`。
- 如果不同值数量小于等于 `breaks`，返回排序后的唯一值。

## 基准测试（PG18）

使用脚本对比 `ST_EvilTransform` 与 `Regex_EvilTransform`：

```bash
# 可选环境变量：PGHOST, PGPORT, PGUSER, PGDATABASE, ROWS
scripts/benchmark_pg18.sh
# 实验性 PG19 beta：
PG_MAJOR=19 scripts/benchmark_pg18.sh
```

脚本会：

- 确保已安装 `postgis` 与 `pg_eviltransform` 扩展。
- 加载 `regex_eviltransform.sql`（创建 `Regex_EvilTransform`）。
- 在 `public.bench_points` 生成测试数据。
- 对 `ST_EvilTransform` 与 `Regex_EvilTransform` 执行 `EXPLAIN (ANALYZE, BUFFERS)`。
- 把报告写入 `benchmark_pg18_report.txt`（或自定义路径）。

示例：

```bash
ROWS=500000 PGDATABASE=testdb scripts/benchmark_pg18.sh /tmp/bench_pg18.txt
```

最新结果（PG18，`ROWS=200000`，报告文件：`benchmark_pg18_report.txt`）：

| 场景 | `ST_EvilTransform` | `Regex_EvilTransform` | 速度比（`Regex` / `ST`） |
|---|---:|---:|---:|
| `4326 -> 990001` | `92.402 ms` | `2832.821 ms` | `30.7x` |
| `990002 -> 3857 (via 4326)` | `183.856 ms` | `8393.272 ms` | `45.7x` |

实验性 PG19 beta 结果（`postgres:19beta1-trixie`，`ROWS=200000`，报告文件：`benchmark_pg19_report.txt`）：

| 场景 | `ST_EvilTransform` | `Regex_EvilTransform` | 速度比（`Regex` / `ST`） |
|---|---:|---:|---:|
| `4326 -> 990001` | `100.307 ms` | `2874.719 ms` | `28.7x` |
| `990002 -> 3857 (via 4326)` | `182.430 ms` | `8230.002 ms` | `45.1x` |

## Jenks 基准测试

使用 `scripts/benchmark_jenksbins.sh` 对比 CartoDB SQL 基线、Rust 数组形式和 Rust 流式聚合形式：

```bash
# 可选环境变量：ROWS, DISTINCT_VALUES, BREAKS, WORK_MEM, PGHOST, PGPORT, PGUSER, PGDATABASE
scripts/benchmark_jenksbins.sh
```

基准测试使用的 CartoDB 基线 SQL 已随仓库放在 `scripts/CDB_JenksBins.sql`，其中包含上游归属和许可证说明。

PG19 beta Docker 结果（`ROWS=100000`，`DISTINCT_VALUES=1000`，`BREAKS=7`，`WORK_MEM=8MB`）：

| 场景 | 执行时间 |
|---|---:|
| `CDB_JenksBins(array_agg(value::numeric), breaks)` | `95.785 ms` |
| `ST_JenksBins(array_agg(value), breaks)` | `11.381 ms` |
| `ST_JenksBins(value, breaks)` 流式聚合 | `8.210 ms` |

流式聚合不需要构造 `array_agg` 输入，并在聚合过程中维护内部的不同值计数表。

## Debian Trixie 发版（PG14-19）

```bash
scripts/release_debian_trixie.sh
```

会在 `dist/` 目录生成 `.deb` 包：

- `postgresql-14-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-15-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-16-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-17-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-18-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-19-pg-eviltransform_<version>_trixie_<arch>.deb`

GitHub Release CI 会同时发布 `amd64` 和 `arm64` 两种架构的包产物（`.deb` + `.rpm`）。

## 许可证

MIT，见 `LICENSE`。
