-- Preserve the public signatures and OIDs while replacing search_path-sensitive SQL.
-- PostGIS and this extension may live in different schemas.
DO $upgrade$
DECLARE
  ext_schema text;
  geometry_schema text;
BEGIN
  SELECT n.nspname INTO STRICT ext_schema
  FROM pg_catalog.pg_extension e
  JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace
  WHERE e.extname = 'pg_eviltransform';
  SELECT n.nspname INTO STRICT geometry_schema
  FROM pg_catalog.pg_extension e
  JOIN pg_catalog.pg_namespace n ON n.oid = e.extnamespace
  WHERE e.extname = 'postgis';

  EXECUTE pg_catalog.format(
    'CREATE OR REPLACE FUNCTION %I.st_eviltransform(geom %I.geometry, to_proj text)
     RETURNS %I.geometry LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
     AS %L, %L', ext_schema, geometry_schema, geometry_schema,
    'MODULE_PATHNAME', 'st_eviltransform_text_wrapper');
  EXECUTE pg_catalog.format(
    'CREATE OR REPLACE FUNCTION %I.st_eviltransform(geom %I.geometry, from_proj text, to_srid integer)
     RETURNS %I.geometry LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
     AS %L, %L', ext_schema, geometry_schema, geometry_schema,
    'MODULE_PATHNAME', 'st_eviltransform_from_text_integer_wrapper');
  EXECUTE pg_catalog.format(
    'CREATE OR REPLACE FUNCTION %I.st_eviltransform(geom %I.geometry, from_proj text, to_proj text)
     RETURNS %I.geometry LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
     AS %L, %L', ext_schema, geometry_schema, geometry_schema,
    'MODULE_PATHNAME', 'st_eviltransform_from_text_wrapper');
END
$upgrade$;
