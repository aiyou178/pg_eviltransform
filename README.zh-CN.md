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

- Rust stable
- `cargo-pgrx`（`cargo install --locked cargo-pgrx --version 0.17.0`）
- PostgreSQL + PostGIS（支持 PG 14-18）
- `librttopo-dev`（用于原生 GSERIALIZED 快速路径）

打包说明：

- DEB 包仅对 PostgreSQL 主包做硬依赖。
- PostGIS 作为建议依赖（`Suggests`），避免 `apt install` 时自动拉取大量 GIS 依赖链。
- 运行时仍需要先安装并创建 PostGIS（`CREATE EXTENSION postgis`），再创建 `pg_eviltransform`。

## 构建

```bash
cargo pgrx init \
  --pg14=/usr/lib/postgresql/14/bin/pg_config \
  --pg15=/usr/lib/postgresql/15/bin/pg_config \
  --pg16=/usr/lib/postgresql/16/bin/pg_config \
  --pg17=/usr/lib/postgresql/17/bin/pg_config \
  --pg18=/usr/lib/postgresql/18/bin/pg_config

cargo pgrx package --release --features pg18 --no-default-features --pg-config /usr/lib/postgresql/18/bin/pg_config
```

## 测试（Rust 原生）

```bash
cargo test
cargo pgrx test pg18 --features pg18
```

## 使用示例

```sql
-- WGS84 (4326) -> GCJ02 (990001)
SELECT ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 990001);

-- BD09 (990002) -> Web Mercator (3857)
SELECT ST_EvilTransform(ST_SetSRID('POINT(120.011070620552 30.0038830555128)'::geometry, 990002), 3857);
```

## 基准测试（PG18）

使用脚本对比 `ST_EvilTransform` 与 `Regex_EvilTransform`：

```bash
# 可选环境变量：PGHOST, PGPORT, PGUSER, PGDATABASE, ROWS
scripts/benchmark_pg18.sh
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

## Debian Trixie 发版（PG14-18）

```bash
scripts/release_debian_trixie.sh
```

会在 `dist/` 目录生成 `.deb` 包：

- `postgresql-14-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-15-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-16-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-17-pg-eviltransform_<version>_trixie_<arch>.deb`
- `postgresql-18-pg-eviltransform_<version>_trixie_<arch>.deb`

GitHub Release CI 会同时发布 `amd64` 和 `arm64` 两种架构的包产物（`.deb` + `.rpm`）。

## 许可证

MIT，见 `LICENSE`。
