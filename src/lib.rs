pub mod coord;
pub mod ewkb;
pub mod jenks;

#[cfg(all(test, feature = "extension"))]
pub mod pg_test {
    pub fn setup(_options: Vec<&str>) {}

    pub fn postgresql_conf_options() -> Vec<&'static str> {
        vec![]
    }
}

#[cfg(feature = "extension")]
pgrx::pg_module_magic!();

#[cfg(feature = "extension")]
#[pgrx::pg_schema]
mod eviltransform_internal {}

#[cfg(feature = "extension")]
mod extension {
    use std::ffi::CString;
    use std::ptr;
    use std::sync::OnceLock;

    use pgrx::Internal;
    use pgrx::datum::AnyElement;
    use pgrx::direct_function_call;
    use pgrx::prelude::*;

    use crate::coord::TransformKind;
    use crate::jenks::{self, JenksCounts};

    const SRID_WGS84: i32 = 4326;
    const SRID_GCJ02: i32 = 990001;
    const SRID_BD09: i32 = 990002;

    const MODE_WGS2GCJ: i32 = 1;
    const MODE_GCJ2WGS: i32 = 2;
    const MODE_WGS2BD: i32 = 3;
    const MODE_BD2WGS: i32 = 4;
    const MODE_GCJ2BD: i32 = 5;
    const MODE_BD2GCJ: i32 = 6;

    #[inline]
    fn transform_bytes(mut input: Vec<u8>, kind: TransformKind) -> Vec<u8> {
        if let Err(err) = crate::ewkb::transform_ewkb_in_place(&mut input, kind) {
            error!("failed to transform EWKB geometry: {err}");
        }
        input
    }

    #[inline]
    fn kind_from_mode(mode: i32) -> TransformKind {
        match mode {
            MODE_WGS2GCJ => TransformKind::Wgs2Gcj,
            MODE_GCJ2WGS => TransformKind::Gcj2Wgs,
            MODE_WGS2BD => TransformKind::Wgs2Bd,
            MODE_BD2WGS => TransformKind::Bd2Wgs,
            MODE_GCJ2BD => TransformKind::Gcj2Bd,
            MODE_BD2GCJ => TransformKind::Bd2Gcj,
            _ => error!("unsupported transform mode: {mode}"),
        }
    }

    #[pg_extern(
        immutable,
        strict,
        parallel_safe,
        name = "__st_eviltransform_ewkb",
        schema = "eviltransform_internal"
    )]
    fn st_eviltransform_ewkb_internal(input: Vec<u8>, mode: i32) -> Vec<u8> {
        transform_bytes(input, kind_from_mode(mode))
    }

    unsafe extern "C" {
        fn pgct_gserialized_get_srid(input: *const u8, out_srid: *mut i32) -> i32;
        fn pgct_transform_gserialized(
            input: *const u8,
            mode: i32,
            dst_srid: i32,
            out_ptr: *mut *mut u8,
            out_len: *mut usize,
        ) -> i32;
        fn pgct_free(ptr: *mut std::ffi::c_void);
    }

    #[derive(Clone, Copy)]
    struct PostgisFns {
        st_transform_geom_int: pg_sys::Oid,
    }

    fn lookup_regprocedure_oid(sig: &str) -> pg_sys::Oid {
        let signature =
            CString::new(sig).unwrap_or_else(|_| error!("invalid regprocedure signature: {sig}"));
        unsafe {
            direct_function_call::<pg_sys::Oid>(
                pg_sys::regprocedurein,
                &[signature.as_c_str().into_datum()],
            )
            .unwrap_or_else(|| error!("regprocedure not found: {sig}"))
        }
    }

    fn postgis_fns() -> &'static PostgisFns {
        static FNS: OnceLock<PostgisFns> = OnceLock::new();
        FNS.get_or_init(|| PostgisFns {
            st_transform_geom_int: lookup_regprocedure_oid("st_transform(geometry,integer)"),
        })
    }

    #[inline]
    unsafe fn call2(oid: pg_sys::Oid, a1: pg_sys::Datum, a2: pg_sys::Datum) -> pg_sys::Datum {
        unsafe { pg_sys::OidFunctionCall2Coll(oid, pg_sys::InvalidOid, a1, a2) }
    }

    fn gserialized_get_srid(geom: pg_sys::Datum) -> i32 {
        let original = geom.cast_mut_ptr::<pg_sys::varlena>();
        let detoasted = unsafe { pg_sys::pg_detoast_datum(original) as *mut pg_sys::varlena };
        if detoasted.is_null() {
            error!("failed to detoast geometry for SRID read");
        }

        let mut srid = 0i32;
        let rc = unsafe { pgct_gserialized_get_srid(detoasted.cast::<u8>(), &mut srid) };
        if detoasted != original {
            unsafe { pg_sys::pfree(detoasted.cast()) };
        }
        if rc != 0 {
            error!("failed to read geometry SRID using librttopo, rc={rc}");
        }
        srid
    }

    fn apply_custom_mode(geom: pg_sys::Datum, mode: i32, dst_srid: i32) -> pg_sys::Datum {
        let original = geom.cast_mut_ptr::<pg_sys::varlena>();
        let detoasted = unsafe { pg_sys::pg_detoast_datum(original) as *mut pg_sys::varlena };
        if detoasted.is_null() {
            error!("failed to detoast geometry for custom transform");
        }

        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len = 0usize;
        let rc = unsafe {
            pgct_transform_gserialized(
                detoasted.cast::<u8>(),
                mode,
                dst_srid,
                &mut out_ptr,
                &mut out_len,
            )
        };

        if detoasted != original {
            unsafe { pg_sys::pfree(detoasted.cast()) };
        }

        if rc != 0 || out_ptr.is_null() || out_len == 0 {
            error!("librttopo transform failed, rc={rc}, len={out_len}");
        }
        if out_len > i32::MAX as usize {
            unsafe { pgct_free(out_ptr.cast()) };
            error!("librttopo transform output too large: {out_len}");
        }

        let out_varlena = unsafe { pg_sys::palloc(out_len) as *mut u8 };
        if out_varlena.is_null() {
            unsafe { pgct_free(out_ptr.cast()) };
            error!("palloc failed for transformed geometry, len={out_len}");
        }

        unsafe {
            ptr::copy_nonoverlapping(out_ptr, out_varlena, out_len);
            pgct_free(out_ptr.cast());
            pgrx::set_varsize_4b(out_varlena.cast::<pg_sys::varlena>(), out_len as i32);
        }

        pg_sys::Datum::from(out_varlena.cast::<pg_sys::varlena>())
    }

    #[pg_extern(immutable, strict, parallel_safe, name = "st_eviltransform")]
    fn st_eviltransform_integer(geom: AnyElement, dst_srid: i32) -> AnyElement {
        let fns = postgis_fns();
        let input = geom.datum();
        let src_srid = gserialized_get_srid(input);

        let result = if src_srid == dst_srid {
            input
        } else if src_srid != SRID_GCJ02
            && src_srid != SRID_BD09
            && dst_srid != SRID_GCJ02
            && dst_srid != SRID_BD09
        {
            unsafe { call2(fns.st_transform_geom_int, input, dst_srid.into()) }
        } else if src_srid == SRID_GCJ02 && dst_srid == SRID_BD09 {
            apply_custom_mode(input, MODE_GCJ2BD, SRID_BD09)
        } else if src_srid == SRID_BD09 && dst_srid == SRID_GCJ02 {
            apply_custom_mode(input, MODE_BD2GCJ, SRID_GCJ02)
        } else if src_srid == SRID_GCJ02 {
            let wgs = apply_custom_mode(input, MODE_GCJ2WGS, SRID_WGS84);
            if dst_srid == SRID_WGS84 {
                wgs
            } else {
                unsafe { call2(fns.st_transform_geom_int, wgs, dst_srid.into()) }
            }
        } else if src_srid == SRID_BD09 {
            let wgs = apply_custom_mode(input, MODE_BD2WGS, SRID_WGS84);
            if dst_srid == SRID_WGS84 {
                wgs
            } else {
                unsafe { call2(fns.st_transform_geom_int, wgs, dst_srid.into()) }
            }
        } else if dst_srid == SRID_GCJ02 {
            let wgs = if src_srid == SRID_WGS84 {
                input
            } else {
                unsafe { call2(fns.st_transform_geom_int, input, SRID_WGS84.into()) }
            };
            apply_custom_mode(wgs, MODE_WGS2GCJ, SRID_GCJ02)
        } else if dst_srid == SRID_BD09 {
            let wgs = if src_srid == SRID_WGS84 {
                input
            } else {
                unsafe { call2(fns.st_transform_geom_int, input, SRID_WGS84.into()) }
            };
            apply_custom_mode(wgs, MODE_WGS2BD, SRID_BD09)
        } else {
            unsafe { call2(fns.st_transform_geom_int, input, dst_srid.into()) }
        };

        unsafe { <AnyElement as FromDatum>::from_polymorphic_datum(result, false, geom.oid()) }
            .unwrap_or_else(|| error!("failed to build transformed geometry datum"))
    }

    fn values_to_jenks<I>(values: I, breaks: i32, invert: bool) -> Option<Vec<f64>>
    where
        I: IntoIterator<Item = f64>,
    {
        match jenks::breaks_from_values(values, breaks, invert) {
            Ok(result) => result,
            Err(err) => error!("{err}"),
        }
    }

    fn anynumeric_to_f64(value: AnyNumeric) -> f64 {
        match f64::try_from(value) {
            Ok(value) if value.is_finite() => value,
            Ok(value) => error!("Jenks input must be finite f64, got {value}"),
            Err(err) => error!("numeric value cannot be converted to finite f64: {err}"),
        }
    }

    fn collect_array_values<T, F>(values: Vec<Option<T>>, convert: F) -> Vec<f64>
    where
        F: Fn(T) -> f64,
    {
        values
            .into_iter()
            .filter_map(|value| value.map(&convert))
            .collect()
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float8_array(values: Vec<Option<f64>>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_float8_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float8_array_invert(
        values: Vec<Option<f64>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(collect_array_values(values, |value| value), breaks, invert)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float4_array(values: Vec<Option<f32>>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_float4_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float4_array_invert(
        values: Vec<Option<f32>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(
            collect_array_values(values, |value| f64::from(value)),
            breaks,
            invert,
        )
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int8_array(values: Vec<Option<i64>>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int8_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int8_array_invert(
        values: Vec<Option<i64>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(
            collect_array_values(values, |value| value as f64),
            breaks,
            invert,
        )
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int4_array(values: Vec<Option<i32>>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int4_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int4_array_invert(
        values: Vec<Option<i32>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(
            collect_array_values(values, |value| f64::from(value)),
            breaks,
            invert,
        )
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int2_array(values: Vec<Option<i16>>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int2_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int2_array_invert(
        values: Vec<Option<i16>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(
            collect_array_values(values, |value| f64::from(value)),
            breaks,
            invert,
        )
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_numeric_array(
        values: Vec<Option<AnyNumeric>>,
        breaks: i32,
    ) -> Option<Vec<f64>> {
        st_jenksbins_numeric_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_numeric_array_invert(
        values: Vec<Option<AnyNumeric>>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        values_to_jenks(
            collect_array_values(values, anynumeric_to_f64),
            breaks,
            invert,
        )
    }

    #[derive(Clone, Default)]
    struct JenksBinsState {
        counts: JenksCounts,
        breaks: i32,
        invert: bool,
        initialized: bool,
    }

    impl JenksBinsState {
        fn add_value(&mut self, value: Option<f64>, breaks: i32, invert: bool) {
            if breaks < 1 {
                error!("breaks must be greater than or equal to 1");
            }
            if self.initialized {
                if self.breaks != breaks || self.invert != invert {
                    error!("ST_JenksBins aggregate breaks and invert arguments must be constant");
                }
            } else {
                self.breaks = breaks;
                self.invert = invert;
                self.initialized = true;
            }
            if let Some(value) = value {
                if let Err(err) = self.counts.push(value) {
                    error!("{err}");
                }
            }
        }

        fn finalize(self) -> Option<Vec<f64>> {
            if !self.initialized {
                return None;
            }
            match jenks::breaks_from_counts(&self.counts, self.breaks, self.invert) {
                Ok(result) => result,
                Err(err) => error!("{err}"),
            }
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbF8;

    #[pg_aggregate]
    impl Aggregate<JbF8> for JbF8 {
        type Args = (Option<f64>, i32);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value, breaks, false);
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.clone().finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbF8Inv;

    #[pg_aggregate]
    impl Aggregate<JbF8Inv> for JbF8Inv {
        type Args = (Option<f64>, i32, bool);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks, invert): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value, breaks, invert);
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.clone().finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbNum;

    #[pg_aggregate]
    impl Aggregate<JbNum> for JbNum {
        type Args = (Option<AnyNumeric>, i32);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value.map(anynumeric_to_f64), breaks, false);
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.clone().finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbNumInv;

    #[pg_aggregate]
    impl Aggregate<JbNumInv> for JbNumInv {
        type Args = (Option<AnyNumeric>, i32, bool);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks, invert): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value.map(anynumeric_to_f64), breaks, invert);
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.clone().finalize())
        }
    }

    extension_sql!(
        r#"
        CREATE FUNCTION eviltransform_internal.__parse_custom_srid(spec text)
        RETURNS integer
        LANGUAGE SQL
        IMMUTABLE STRICT PARALLEL SAFE
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

        CREATE FUNCTION st_eviltransform(geom geometry, to_proj text)
        RETURNS geometry
        LANGUAGE SQL
        IMMUTABLE STRICT PARALLEL SAFE
        AS $$
        WITH params AS (
          SELECT
            geom,
            ST_SRID(geom) AS src_srid,
            eviltransform_internal.__parse_custom_srid(to_proj) AS dst_custom
        )
        SELECT CASE
          WHEN dst_custom IS NOT NULL THEN st_eviltransform(geom, dst_custom)
          WHEN src_srid IN (990001, 990002)
            THEN ST_Transform(st_eviltransform(geom, 4326), to_proj)
          ELSE ST_Transform(geom, to_proj)
        END
        FROM params;
        $$;

        CREATE FUNCTION st_eviltransform(geom geometry, from_proj text, to_srid integer)
        RETURNS geometry
        LANGUAGE SQL
        IMMUTABLE STRICT PARALLEL SAFE
        AS $$
        WITH params AS (
          SELECT
            geom,
            to_srid AS dst_srid,
            eviltransform_internal.__parse_custom_srid(from_proj) AS src_custom
        )
        SELECT CASE
          WHEN src_custom IS NULL AND dst_srid NOT IN (990001, 990002)
            THEN ST_Transform(geom, from_proj, dst_srid)
          WHEN src_custom IS NOT NULL
            THEN st_eviltransform(ST_SetSRID(geom, src_custom), dst_srid)
          ELSE st_eviltransform(ST_Transform(geom, from_proj, 4326), dst_srid)
        END
        FROM params;
        $$;

        CREATE FUNCTION st_eviltransform(geom geometry, from_proj text, to_proj text)
        RETURNS geometry
        LANGUAGE SQL
        IMMUTABLE STRICT PARALLEL SAFE
        AS $$
        WITH params AS (
          SELECT
            geom,
            eviltransform_internal.__parse_custom_srid(from_proj) AS src_custom,
            eviltransform_internal.__parse_custom_srid(to_proj) AS dst_custom
        )
        SELECT CASE
          WHEN src_custom IS NULL AND dst_custom IS NULL
            THEN ST_Transform(geom, from_proj, to_proj)
          WHEN src_custom IS NOT NULL AND dst_custom IS NOT NULL
            THEN st_eviltransform(ST_SetSRID(geom, src_custom), dst_custom)
          WHEN src_custom IS NOT NULL
            THEN ST_Transform(st_eviltransform(ST_SetSRID(geom, src_custom), 4326), to_proj)
          ELSE st_eviltransform(ST_Transform(geom, from_proj, 4326), dst_custom)
        END
        FROM params;
        $$;
        "#,
        name = "st_eviltransform_sql",
        requires = [st_eviltransform_integer, st_eviltransform_ewkb_internal]
    );

    #[cfg(any(test, feature = "pg_test"))]
    #[pg_schema]
    mod tests {
        use pgrx::prelude::*;

        #[pg_test]
        fn test_integer_overload_to_gcj02() {
            let got = Spi::get_one::<String>(&format!(
                "SELECT ST_AsText(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, {}), {}))",
                super::SRID_WGS84,
                super::SRID_GCJ02
            ))
            .expect("SPI failed")
            .expect("no row returned");

            assert!(got.starts_with("POINT("));
            assert!(got.contains("120.004660445597"));
        }

        #[pg_test]
        fn test_delegates_to_st_transform_for_standard_srids() {
            let got = Spi::get_one::<bool>(
                "SELECT ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 3857)) = ST_AsEWKB(ST_Transform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 3857))",
            )
            .expect("SPI failed")
            .expect("no row returned");

            assert!(got);
        }

        #[pg_test]
        fn test_text_to_proj_overload_custom_target() {
            let got = Spi::get_one::<i32>(&format!(
                "SELECT ST_SRID(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, {}), 'GCJ02'))",
                super::SRID_WGS84
            ))
            .expect("SPI failed")
            .expect("no row returned");

            assert_eq!(got, super::SRID_GCJ02);
        }

        #[pg_test]
        fn test_from_proj_to_srid_overload_custom_target() {
            let got = Spi::get_one::<i32>(
                "SELECT ST_SRID(ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 990002))",
            )
            .expect("SPI failed")
            .expect("no row returned");

            assert_eq!(got, super::SRID_BD09);
        }

        #[pg_test]
        fn test_source_geometry_not_mutated() {
            Spi::run(&format!(
                "CREATE TEMP TABLE t AS SELECT ST_SetSRID('POINT(120 30)'::geometry, {}) AS g",
                super::SRID_WGS84
            ))
            .expect("failed to create temp table");

            let _ = Spi::get_one::<String>(&format!(
                "SELECT encode(ST_AsEWKB(ST_EvilTransform(g, {})), 'hex') FROM t",
                super::SRID_BD09
            ))
            .expect("transform query failed");

            let original = Spi::get_one::<String>("SELECT ST_AsText(g) FROM t")
                .expect("readback failed")
                .expect("no original geometry");

            assert_eq!(original, "POINT(120 30)");
        }

        #[pg_test]
        fn test_direct_gcj_bd_mode_matches_core_transform() {
            let got = Spi::get_one::<bool>(
                "SELECT ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 990001), 990002)) = ST_AsEWKB(ST_SetSRID(ST_GeomFromEWKB(eviltransform_internal.__st_eviltransform_ewkb(ST_AsEWKB(ST_SetSRID('POINT(120 30)'::geometry, 990001)), 5)), 990002))",
            )
            .expect("SPI failed")
            .expect("no row returned");

            assert!(got);
        }

        #[pg_test]
        fn test_jenksbins_array_overloads() {
            for sql in [
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::numeric[], 2) = ARRAY[2, 11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::double precision[], 2) = ARRAY[2, 11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::real[], 2) = ARRAY[2, 11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::smallint[], 2) = ARRAY[2, 11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::integer[], 2) = ARRAY[2, 11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1, 2, NULL, 10, 11]::bigint[], 2) = ARRAY[2, 11]::double precision[]",
            ] {
                let got = Spi::get_one::<bool>(sql)
                    .expect("SPI failed")
                    .expect("no row returned");
                assert!(got, "{sql}");
            }
        }

        #[pg_test]
        fn test_jenksbins_aggregate_matches_array() {
            let got = Spi::get_one::<bool>(
                "WITH data(value) AS (
                   SELECT unnest(ARRAY[1, 1, 2, 2, 10, 11, NULL]::double precision[])
                 )
                 SELECT ST_JenksBins(value, 2) = ST_JenksBins(array_agg(value), 2)
                 FROM data",
            )
            .expect("SPI failed")
            .expect("no row returned");
            assert!(got);
        }

        #[pg_test]
        fn test_jenksbins_numeric_aggregate_and_invert() {
            let got = Spi::get_one::<bool>(
                "WITH data(value) AS (
                   SELECT unnest(ARRAY[1, 2, 3, 10, 11, 12]::numeric[])
                 )
                 SELECT ST_JenksBins(value, 2, true) = ARRAY[1, 10]::double precision[]
                 FROM data",
            )
            .expect("SPI failed")
            .expect("no row returned");
            assert!(got);
        }

        #[pg_test]
        fn test_jenksbins_duplicate_heavy_large_aggregate() {
            let got = Spi::get_one::<bool>(
                "WITH data AS (
                   SELECT (g % 100)::double precision AS value
                   FROM generate_series(1, 10000) AS g
                 )
                 SELECT cardinality(ST_JenksBins(value, 5)) = 5
                 FROM data",
            )
            .expect("SPI failed")
            .expect("no row returned");
            assert!(got);
        }
    }
}
