-- Regex_EvilTransform
-- Regex-based coordinate transform extension for PostGIS geometries.
-- Patent citation: this regex SQL implementation references CN112000902B:
-- https://patents.google.com/patent/CN112000902B/zh

CREATE SCHEMA IF NOT EXISTS regex_eviltransform_internal;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__wgs2gcj(
  wgs_lng double precision,
  wgs_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  a constant double precision := 6378245.0;
  ee constant double precision := 0.00669342162296594323;
  d_lat double precision;
  d_lng double precision;
  x double precision;
  y double precision;
  rad_lat double precision;
  magic double precision;
  sqrt_magic double precision;
BEGIN
  IF (wgs_lng < 72.004 OR wgs_lng > 137.8347 OR wgs_lat < 0.8293 OR wgs_lat > 55.8271) THEN
    lng := wgs_lng;
    lat := wgs_lat;
    RETURN;
  END IF;

  x := wgs_lng - 105.0;
  y := wgs_lat - 35.0;

  d_lat := -100.0 + 2.0 * x + 3.0 * y + 0.2 * power(y, 2) + 0.1 * x * y + 0.2 * sqrt(abs(x))
    + (20.0 * sin(6.0 * x * pi()) + 20.0 * sin(2.0 * x * pi())) * 2.0 / 3.0
    + (20.0 * sin(y * pi()) + 40.0 * sin(y / 3.0 * pi())) * 2.0 / 3.0
    + (160.0 * sin(y / 12.0 * pi()) + 320.0 * sin(y * pi() / 30.0)) * 2.0 / 3.0;

  d_lng := 300.0 + x + 2.0 * y + 0.1 * power(x, 2) + 0.1 * x * y + 0.1 * sqrt(abs(x))
    + (20.0 * sin(6.0 * x * pi()) + 20.0 * sin(2.0 * x * pi())) * 2.0 / 3.0
    + (20.0 * sin(x * pi()) + 40.0 * sin(x / 3.0 * pi())) * 2.0 / 3.0
    + (150.0 * sin(x / 12.0 * pi()) + 300.0 * sin(x / 30.0 * pi())) * 2.0 / 3.0;

  rad_lat := wgs_lat / 180.0 * pi();
  magic := sin(rad_lat);
  magic := 1 - ee * magic * magic;
  sqrt_magic := sqrt(magic);
  d_lng := (d_lng * 180.0) / (a / sqrt_magic * cos(rad_lat) * pi());
  d_lat := (d_lat * 180.0) / ((a * (1 - ee)) / (magic * sqrt_magic) * pi());

  lng := wgs_lng + d_lng;
  lat := wgs_lat + d_lat;
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__gcj2wgs(
  gcj_lng double precision,
  gcj_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  p record;
BEGIN
  SELECT * INTO p FROM regex_eviltransform_internal.__wgs2gcj(gcj_lng, gcj_lat);
  lng := gcj_lng - (p.lng - gcj_lng);
  lat := gcj_lat - (p.lat - gcj_lat);
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__gcj2bd(
  gcj_lng double precision,
  gcj_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  z double precision;
  theta double precision;
  x_pi constant double precision := 3.14159265358979324 * 3000.0 / 180.0;
BEGIN
  z := sqrt(power(gcj_lng, 2) + power(gcj_lat, 2)) + 0.00002 * sin(gcj_lat * x_pi);
  theta := atan2(gcj_lat, gcj_lng) + 0.000003 * cos(gcj_lng * x_pi);
  lng := z * cos(theta) + 0.0065;
  lat := z * sin(theta) + 0.006;
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__bd2gcj(
  bd_lng double precision,
  bd_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  x double precision;
  y double precision;
  z double precision;
  theta double precision;
  x_pi constant double precision := 3.14159265358979324 * 3000.0 / 180.0;
BEGIN
  x := bd_lng - 0.0065;
  y := bd_lat - 0.006;
  z := sqrt(power(x, 2) + power(y, 2)) - 0.00002 * sin(y * x_pi);
  theta := atan2(y, x) - 0.000003 * cos(x * x_pi);
  lng := z * cos(theta);
  lat := z * sin(theta);
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__wgs2bd(
  wgs_lng double precision,
  wgs_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  p record;
BEGIN
  SELECT * INTO p FROM regex_eviltransform_internal.__wgs2gcj(wgs_lng, wgs_lat);
  SELECT * INTO p FROM regex_eviltransform_internal.__gcj2bd(p.lng, p.lat);
  lng := p.lng;
  lat := p.lat;
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__bd2wgs(
  bd_lng double precision,
  bd_lat double precision,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  p record;
BEGIN
  SELECT * INTO p FROM regex_eviltransform_internal.__bd2gcj(bd_lng, bd_lat);
  SELECT * INTO p FROM regex_eviltransform_internal.__gcj2wgs(p.lng, p.lat);
  lng := p.lng;
  lat := p.lat;
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__parse_custom_srid(spec text)
RETURNS integer
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
SELECT CASE upper(trim(spec))
  WHEN '990001' THEN 990001
  WHEN 'EPSG:990001' THEN 990001
  WHEN 'GCJ02' THEN 990001
  WHEN 'GCJ-02' THEN 990001
  WHEN '990002' THEN 990002
  WHEN 'EPSG:990002' THEN 990002
  WHEN 'BD09' THEN 990002
  WHEN 'BD-09' THEN 990002
  ELSE NULL
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__apply_scalar(
  src_lng double precision,
  src_lat double precision,
  mode integer,
  OUT lng double precision,
  OUT lat double precision
)
RETURNS record
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
BEGIN
  CASE mode
    WHEN 1 THEN SELECT * INTO lng, lat FROM regex_eviltransform_internal.__wgs2gcj(src_lng, src_lat);
    WHEN 2 THEN SELECT * INTO lng, lat FROM regex_eviltransform_internal.__gcj2wgs(src_lng, src_lat);
    WHEN 3 THEN SELECT * INTO lng, lat FROM regex_eviltransform_internal.__wgs2bd(src_lng, src_lat);
    WHEN 4 THEN SELECT * INTO lng, lat FROM regex_eviltransform_internal.__bd2wgs(src_lng, src_lat);
    ELSE RAISE EXCEPTION 'unsupported mode: %', mode;
  END CASE;
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__apply_geometry(
  input_geometry geometry,
  mode integer,
  output_srid integer
)
RETURNS geometry
LANGUAGE plpgsql
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
DECLARE
  item record;
  transformed record;
  wkt_template text;
  points double precision[] := '{}'::double precision[];
BEGIN
  IF ST_IsEmpty(input_geometry) THEN
    RETURN ST_SetSRID(input_geometry, output_srid);
  END IF;

  IF GeometryType(input_geometry) = 'POINT' THEN
    SELECT * INTO transformed
    FROM regex_eviltransform_internal.__apply_scalar(ST_X(input_geometry), ST_Y(input_geometry), mode);

    RETURN ST_SetSRID(ST_MakePoint(transformed.lng, transformed.lat), output_srid);
  END IF;

  wkt_template := ST_AsText(input_geometry);
  wkt_template := regexp_replace(
    wkt_template,
    '[-+]?[0-9]*\.?[0-9]+([eE][-+]?[0-9]+)?',
    '%s',
    'g'
  );

  FOR item IN
    SELECT (dp).geom AS geom
    FROM (SELECT ST_DumpPoints(input_geometry) AS dp) t
  LOOP
    SELECT * INTO transformed
    FROM regex_eviltransform_internal.__apply_scalar(ST_X(item.geom), ST_Y(item.geom), mode);

    points := points || transformed.lng || transformed.lat;
  END LOOP;

  RETURN ST_SetSRID(ST_GeomFromText(format(wkt_template, variadic points)), output_srid);
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__to_wgs84(geom geometry, src_srid integer)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
SELECT CASE
  WHEN src_srid = 4326 THEN geom
  WHEN src_srid = 990001 THEN regex_eviltransform_internal.__apply_geometry(geom, 2, 4326)
  WHEN src_srid = 990002 THEN regex_eviltransform_internal.__apply_geometry(geom, 4, 4326)
  ELSE ST_Transform(geom, 4326)
END;
$$;

CREATE OR REPLACE FUNCTION regex_eviltransform_internal.__from_wgs84(geom_wgs geometry, dst_srid integer)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
SELECT CASE
  WHEN dst_srid = 4326 THEN geom_wgs
  WHEN dst_srid = 990001 THEN regex_eviltransform_internal.__apply_geometry(geom_wgs, 1, 990001)
  WHEN dst_srid = 990002 THEN regex_eviltransform_internal.__apply_geometry(geom_wgs, 3, 990002)
  ELSE ST_Transform(geom_wgs, dst_srid)
END;
$$;

CREATE OR REPLACE FUNCTION Regex_EvilTransform(geom geometry, dst_srid integer)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
SELECT CASE
  WHEN ST_SRID(geom) = dst_srid THEN geom
  WHEN ST_SRID(geom) NOT IN (990001, 990002) AND dst_srid NOT IN (990001, 990002)
    THEN ST_Transform(geom, dst_srid)
  ELSE regex_eviltransform_internal.__from_wgs84(
      regex_eviltransform_internal.__to_wgs84(geom, ST_SRID(geom)),
      dst_srid
    )
END;
$$;

CREATE OR REPLACE FUNCTION Regex_EvilTransform(geom geometry, to_proj text)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
WITH params AS (
  SELECT
    geom,
    ST_SRID(geom) AS src_srid,
    regex_eviltransform_internal.__parse_custom_srid(to_proj) AS dst_custom
)
SELECT CASE
  WHEN dst_custom IS NOT NULL THEN Regex_EvilTransform(geom, dst_custom)
  WHEN src_srid IN (990001, 990002)
    THEN ST_Transform(regex_eviltransform_internal.__to_wgs84(geom, src_srid), to_proj)
  ELSE ST_Transform(geom, to_proj)
END
FROM params;
$$;

CREATE OR REPLACE FUNCTION Regex_EvilTransform(geom geometry, from_proj text, to_srid integer)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
WITH params AS (
  SELECT
    geom,
    to_srid AS dst_srid,
    regex_eviltransform_internal.__parse_custom_srid(from_proj) AS src_custom
)
SELECT CASE
  WHEN src_custom IS NULL AND dst_srid NOT IN (990001, 990002)
    THEN ST_Transform(geom, from_proj, dst_srid)
  WHEN src_custom IS NOT NULL
    THEN Regex_EvilTransform(ST_SetSRID(geom, src_custom), dst_srid)
  ELSE Regex_EvilTransform(ST_Transform(geom, from_proj, 4326), dst_srid)
END
FROM params;
$$;

CREATE OR REPLACE FUNCTION Regex_EvilTransform(geom geometry, from_proj text, to_proj text)
RETURNS geometry
LANGUAGE SQL
IMMUTABLE
STRICT
PARALLEL SAFE
AS $$
WITH params AS (
  SELECT
    geom,
    regex_eviltransform_internal.__parse_custom_srid(from_proj) AS src_custom,
    regex_eviltransform_internal.__parse_custom_srid(to_proj) AS dst_custom
)
SELECT CASE
  WHEN src_custom IS NULL AND dst_custom IS NULL
    THEN ST_Transform(geom, from_proj, to_proj)
  WHEN src_custom IS NOT NULL AND dst_custom IS NOT NULL
    THEN Regex_EvilTransform(ST_SetSRID(geom, src_custom), dst_custom)
  WHEN src_custom IS NOT NULL
    THEN ST_Transform(Regex_EvilTransform(ST_SetSRID(geom, src_custom), 4326), to_proj)
  ELSE Regex_EvilTransform(ST_Transform(geom, from_proj, 4326), dst_custom)
END
FROM params;
$$;
