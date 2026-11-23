\set ON_ERROR_STOP on
-- Run only in a disposable database: this exercises extension recreation.
CREATE SCHEMA gis;
CREATE SCHEMA evil;
CREATE EXTENSION postgis SCHEMA gis;
SET search_path = evil, gis, public;
CREATE EXTENSION pg_eviltransform SCHEMA evil;

-- Resolve functions with neither extension schema on the caller's search_path.
SET search_path = pg_catalog;
DO $$
DECLARE
  input gis.geometry := 'SRID=4326;POINT(120 30)'::gis.geometry;
  output gis.geometry;
BEGIN
  FOREACH output IN ARRAY ARRAY[
    evil.st_eviltransform(input, 990001),
    evil.st_eviltransform(input, 'GCJ02'),
    evil.st_eviltransform(input, 'EPSG:4326', 990001),
    evil.st_eviltransform(input, 'EPSG:4326', 'GCJ02')
  ] LOOP
    IF gis.st_srid(output) <> 990001
       OR abs(gis.st_x(output) - 120.004660445597) > 1e-6
       OR abs(gis.st_y(output) - 29.997534331696) > 1e-6 THEN
      RAISE EXCEPTION 'restricted search_path changed coordinate results';
    END IF;
  END LOOP;
  output := evil.st_eviltransform(input, '+proj=longlat +datum=WGS84 +no_defs');
  IF gis.st_srid(output) <> 0 OR gis.st_x(output) <> 120 OR gis.st_y(output) <> 30 THEN
    RAISE EXCEPTION 'text PostGIS delegation failed';
  END IF;
END $$;

-- Parallel workers must resolve PostGIS read-only without assigning transaction IDs.
DO $$
DECLARE
  c1 bigint;
  c2 bigint;
  c3 bigint;
  c4 bigint;
BEGIN
  IF current_setting('server_version_num')::int >= 160000 THEN
    PERFORM set_config('debug_parallel_query', 'on', true);
  ELSE
    PERFORM set_config('force_parallel_mode', 'on', true);
  END IF;
  WITH pts AS (
    SELECT gis.st_setsrid(gis.st_makepoint(120 + g * 0.001, 30 + g * 0.001), 4326) AS g
    FROM pg_catalog.generate_series(1, 50) AS g
  )
  SELECT
    count(*) FILTER (WHERE gis.st_srid(evil.st_eviltransform(g, 990001)) = 990001),
    count(*) FILTER (WHERE gis.st_srid(evil.st_eviltransform(g, 'BD09')) = 990002),
    count(*) FILTER (WHERE gis.st_srid(evil.st_eviltransform(g, 'EPSG:4326', 990001)) = 990001),
    count(*) FILTER (WHERE gis.st_srid(evil.st_eviltransform(g, 'EPSG:4326', 'BD09')) = 990002)
  INTO c1, c2, c3, c4
  FROM pts;
  IF c1 <> 50 OR c2 <> 50 OR c3 <> 50 OR c4 <> 50 THEN
    RAISE EXCEPTION 'parallel worker transform count mismatch: %, %, %, %', c1, c2, c3, c4;
  END IF;
END $$;

-- Box2D/envelope functions may recompute bounds; &&/&&& read the serialized cache.
DO $$
DECLARE
  output gis.geometry;
BEGIN
  output := evil.st_eviltransform(gis.postgis_addbbox(
    'SRID=4326;LINESTRING ZM (120 30 7 8,121 31 9 10)'::gis.geometry), 990001);
  IF NOT (output OPERATOR(gis.&&) gis.st_endpoint(output))
     OR NOT (output OPERATOR(gis.&&&) gis.st_endpoint(output)) THEN
    RAISE EXCEPTION 'transformed geometry has stale cached bounds';
  END IF;
END $$;

-- Namespace changes invalidate cached schema names without changing type OIDs.
-- PostGIS SQL-language text overloads embed the installation schema in their own
-- bodies, so test them before rename and after recreating PostGIS, while testing
-- prepared call-site invalidation across ALTER SCHEMA on C-backed overloads.
SELECT gis.st_x(evil.st_eviltransform('SRID=4326;POINT(120 30)'::gis.geometry, '+proj=longlat +datum=WGS84 +no_defs'));
PREPARE evil_int_proj(gis.geometry) AS
  SELECT evil.st_eviltransform($1, 3857);
PREPARE evil_custom_proj(gis.geometry) AS
  SELECT evil.st_eviltransform($1, 'BD09');
EXECUTE evil_int_proj('SRID=4326;POINT(120 30)'::gis.geometry);
EXECUTE evil_custom_proj('SRID=4326;POINT(120 30)'::gis.geometry);
ALTER SCHEMA gis RENAME TO gis_renamed;
SELECT evil.st_eviltransform('SRID=4326;POINT(120 30)'::gis_renamed.geometry, 3857);
SELECT evil.st_eviltransform('SRID=4326;POINT(120 30)'::gis_renamed.geometry, 'BD09');
EXECUTE evil_int_proj('SRID=4326;POINT(120 30)'::gis_renamed.geometry);
EXECUTE evil_custom_proj('SRID=4326;POINT(120 30)'::gis_renamed.geometry);
DEALLOCATE evil_int_proj;
DEALLOCATE evil_custom_proj;

-- Recreating PostGIS changes its type/function OIDs in this same backend.
DROP EXTENSION pg_eviltransform;
DROP EXTENSION postgis;
CREATE SCHEMA rebuilt_gis;
CREATE EXTENSION postgis SCHEMA rebuilt_gis;
SET search_path = evil, rebuilt_gis, public;
CREATE EXTENSION pg_eviltransform SCHEMA evil;
SET search_path = pg_catalog;
DO $$
DECLARE
  output rebuilt_gis.geometry;
BEGIN
  output := evil.st_eviltransform('SRID=4326;POINT(120 30)'::rebuilt_gis.geometry, 3857);
  IF rebuilt_gis.st_srid(output) <> 3857
     OR abs(rebuilt_gis.st_x(output) - 13358338.895192828) > 1e-6 THEN
    RAISE EXCEPTION 'stale PostGIS function/type OID after recreation';
  END IF;
  output := evil.st_eviltransform('SRID=4326;POINT(120 30)'::rebuilt_gis.geometry, '+proj=longlat +datum=WGS84 +no_defs');
  IF rebuilt_gis.st_srid(output) <> 0
     OR rebuilt_gis.st_x(output) <> 120 OR rebuilt_gis.st_y(output) <> 30 THEN
    RAISE EXCEPTION 'stale PostGIS text function OID after recreation';
  END IF;
END $$;

SET search_path = evil, rebuilt_gis, public;
\ir ../regex_eviltransform.sql
DO $$
DECLARE
  source geometry;
  output geometry;
  pair record;
BEGIN
  FOREACH source IN ARRAY ARRAY[
    'SRID=4326;POINT Z (120 30 7)'::geometry,
    'SRID=4326;POINT M (120 30 8)'::geometry,
    'SRID=4326;POINT ZM (120 30 7 8)'::geometry,
    'SRID=4326;LINESTRING ZM (120 30 7 8,121 31 9 10)'::geometry,
    'SRID=4326;LINESTRING Z (-12 3 7,-11 4 9)'::geometry,
    'SRID=4326;LINESTRING M (-12 3 8,-11 4 10)'::geometry
  ] LOOP
    output := regex_eviltransform(source, 990001);
    IF st_zmflag(output) <> st_zmflag(source) OR st_npoints(output) <> st_npoints(source) THEN
      RAISE EXCEPTION 'regex transform lost geometry dimensions';
    END IF;
    FOR pair IN
      SELECT a.geom AS original, b.geom AS transformed
      FROM st_dumppoints(source) a JOIN st_dumppoints(output) b USING (path)
    LOOP
      IF st_z(pair.original) IS DISTINCT FROM st_z(pair.transformed)
         OR st_m(pair.original) IS DISTINCT FROM st_m(pair.transformed) THEN
        RAISE EXCEPTION 'regex transform modified Z/M ordinates';
      END IF;
    END LOOP;
  END LOOP;
END $$;
