<p align="center">
  <img src="assets/logo/pg-eviltransform-512.png" alt="pg_eviltransform logo" width="160" height="160">
</p>

# pg_eviltransform

[English](README.md) | [Logo 资源](assets/logo)

为 PostGIS 增加 GCJ02、BD09、CGCS2000 坐标转换，以及精确的 Jenks 自然断点分箱。

## 安装

依赖 PostgreSQL 和 PostGIS。当前源码构建支持：

| PostgreSQL | 最低运行时及构建头文件版本 |
| --- | --- |
| 15 | 15.20 |
| 16 | 16.16 |
| 17 | 17.12 |
| 18 | 18.7 |
| 19 | 19rc2，实验支持 |

这些最低版本包含 pgrx 0.20.0 使用的 PostgreSQL 安全修复，当前源码不再支持 PG14。
二进制包的支持范围取决于发行版本：[pgext.cloud 目录](https://pgext.cloud/ext/pg_eviltransform)
目前列出的是较早的 0.0.7 / pgrx 0.19.3 构建，覆盖 PG14-18。
安装前请核对目录中的 PostgreSQL、操作系统和架构矩阵。

### 安装服务器软件包

安装 [pig](https://pig.pgsty.com/zh) 后，启用目录指定的软件仓库，
再安装对应 PostgreSQL 主版本的包。以下以 PG18 为例：

```bash
pig repo add pgsql -u
pig install pg_eviltransform -v 18
```

同一仓库配置完成后，也可以使用系统包管理器：

```bash
sudo apt install postgresql-18-eviltransform  # Debian / Ubuntu
sudo dnf install pg_eviltransform_18          # RPM 发行版
```

也可以下载 [GitHub Releases](https://github.com/aiyou178/pg_eviltransform/releases)
中匹配环境的二进制包，或[从源码构建](#开发)。上游 DEB 文件使用
`postgresql-18-pg-eviltransform_*.deb` 命名，与目录中的包名不同。

### 在数据库中启用

安装服务器软件包后，由数据库管理员执行：

```sql
CREATE EXTENSION IF NOT EXISTS postgis;
CREATE EXTENSION pg_eviltransform;
SELECT extversion FROM pg_extension WHERE extname = 'pg_eviltransform';
```

无须配置 `shared_preload_libraries`。应用角色可以调用已安装的 API；
文本重载还需要对 `eviltransform_internal` schema 拥有 `USAGE` 权限。

## 用法

### 坐标转换

`ST_EvilTransform` 与 `ST_Transform` 保持四种重载形式：

```text
ST_EvilTransform(geometry, to_srid integer)
ST_EvilTransform(geometry, to_proj text)
ST_EvilTransform(geometry, from_proj text, to_srid integer)
ST_EvilTransform(geometry, from_proj text, to_proj text)
```

| 坐标系 | SRID | 文本别名 |
| --- | --- | --- |
| WGS84 | 4326 | `EPSG:4326` |
| GCJ02 | 990001 | `GCJ02`、`GCJ-02`、`EPSG:990001` |
| BD09 | 990002 | `BD09`、`BD-09`、`EPSG:990002` |
| 地理坐标系 CGCS2000 | 4490 | `CGCS2000`、`CGCS-2000`、`EPSG:4490` |

```sql
-- WGS84 转 GCJ02 或 BD09
SELECT ST_EvilTransform('SRID=4326;POINT(120 30)'::geometry, 'GCJ02');
SELECT ST_EvilTransform('SRID=4326;POINT(120 30)'::geometry, 990002);

-- BD09 转 Web Mercator
SELECT ST_EvilTransform('SRID=990002;POINT(120.01107 30.00388)'::geometry, 3857);

-- 显式指定源坐标系和目标坐标系
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 'GCJ02');
SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'CGCS2000', 990001);
```

标准坐标系委托给 PostGIS/PROJ；自定义坐标系在需要时通过 WGS84 桥接。
CGCS2000 别名表示地理坐标系 EPSG:4490，高斯-克吕格投影分带应使用对应的明确 EPSG 编号。

### Jenks 自然断点

`ST_JenksBins` 返回 `double precision[]` 分箱边界：

```sql
SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::double precision[], 2);
-- {2,11}

SELECT ST_JenksBins(value::double precision, 7) FROM measurements;
SELECT ST_JenksBins(value::double precision, 7, true) FROM measurements;
-- true 返回下边界，默认返回上边界
```

数组支持 `numeric`、`double precision`、`real`、`bigint`、`integer` 和 `smallint`；
聚合支持 `numeric` 和 `double precision`。忽略 NULL，空输入返回 NULL；
非有限数或小于 1 的分箱数会报错。不同值数量不超过分箱数时，返回排序后的去重值。
`numeric` 输入统一以有限 f64 值计算。

## 基准测试

以下为 ARM64 本地结果：pgrx 0.20.0、PostGIS 3.7.0，关闭 JIT 和并行查询，
预热一次后取五次运行的中位数。坐标测试使用 200,000 个点。

| PostgreSQL | 转换路径 | 原生实现 | Regex SQL |
| --- | --- | ---: | ---: |
| 18.7 | 4326 -> GCJ02 | 120.61 ms | 5546.00 ms |
| 18.7 | 4326 -> BD09 -> 3857 | 320.75 ms | 15343.10 ms |
| 19rc2 | 4326 -> GCJ02 | 117.47 ms | 5457.14 ms |
| 19rc2 | 4326 -> BD09 -> 3857 | 321.29 ms | 14996.98 ms |

Jenks 测试使用 100,000 行、1,000 个不同值、7 类，`work_mem=8MB`：

| PostgreSQL | Rust 数组 | Rust 聚合 | CartoDB SQL |
| --- | ---: | ---: | ---: |
| 18.7 | 10.49 ms | 9.11 ms | 84.07 ms |
| 19rc2 | 9.25 ms | 8.36 ms | 82.90 ms |

[完整报告](doc/benchmarks/2026-11-13-pgrx-0.20.0.md)包含升级前后对比、计时范围、
原始执行计划，以及 SQL 基线与原生函数之间的实现差异。结果取决于负载和运行环境。

在临时数据库中复现（脚本会重建基准表）：

```bash
PGDATABASE=bench ROWS=200000 PG_MAJOR=18 scripts/benchmark_pg18.sh /tmp/coordinates.txt
PGDATABASE=bench ROWS=100000 DISTINCT_VALUES=1000 BREAKS=7 WORK_MEM=8MB \
  scripts/benchmark_jenksbins.sh /tmp/jenks.txt
```

## 开发

工具链：Rust **1.99.1**、cargo-pgrx **0.20.0**、主版本一致的 Clang/libclang/LLVM，
以及 PostgreSQL 服务器头文件、PostGIS、ICU、GEOS 和 librttopo 开发库。
Debian 构建环境见[基准镜像 Dockerfile](docker/Dockerfile.benchmark-trixie)。

```bash
rustup toolchain install 1.99.1 --profile minimal
cargo install --locked cargo-pgrx --version 0.20.0
cargo pgrx init --pg18=/usr/lib/postgresql/18/bin/pg_config
cargo test --locked --lib
cargo pgrx package --release --no-default-features --features pg18 \
  --pg-config /usr/lib/postgresql/18/bin/pg_config
cargo pgrx test pg18 --release --no-default-features --features pg_test
```

使用其他主版本时，替换 `pg18` 和头文件路径。使用发行版 PostgreSQL 运行测试时，
需要有安装扩展文件的权限；以 root/容器运行时，在测试命令后添加
`--runas postgres --pgdata /tmp/pg-eviltransform-tests`。

CI 矩阵覆盖 **PG15-19、amd64 和 arm64** 的构建、测试和基准。
Release 构建保留可移植 CPU 目标，使用 `opt-level=3`、fat LTO 和 C `-O3`。

```bash
scripts/release_debian_trixie.sh  # Docker 构建 PG15-19 DEB，输出至 dist/
```

标签触发的发版工作流还会生成 RPM。

## 致谢与许可证

- [googollee/eviltransform](https://github.com/googollee/eviltransform)：坐标转换公式。
- [PostGIS](https://postgis.net/) 和 [librttopo](https://git.osgeo.org/gitea/rttopo/librttopo)：几何处理。
- [pgrx](https://github.com/pgcentralfoundation/pgrx) 和 [natural-breaks](https://github.com/uratne/natural-breaks)：Rust 扩展框架和 Jenks 实现。
- [CartoDB CDB_JenksBins](https://github.com/CartoDB/cartodb-postgresql/blob/master/scripts-available/CDB_JenksBins.sql)：仅用于基准的 SQL 实现，版权归 2014 Vizzuality，采用 BSD 3-Clause。
- [CN112000902B](https://patents.google.com/patent/CN112000902B/zh)：`regex_eviltransform.sql` 的参考专利；使用该 SQL 方案时请引用。

项目源码采用 [MIT 许可证](LICENSE)。librttopo 采用 GPL-2.0-or-later，
第三方组件保留各自的许可证。
