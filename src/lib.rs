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
    use std::cell::Cell;
    use std::ffi::{CStr, CString};
    use std::ptr;
    use std::sync::OnceLock;
    use std::sync::atomic::{AtomicU64, Ordering};

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
        fn pgct_can_transform_point_in_place(input: *const u8, len: usize) -> i32;
        fn pgct_transform_point_in_place(
            buf: *mut u8,
            len: usize,
            mode: i32,
            dst_srid: i32,
        ) -> i32;
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
        st_transform_geom_text: pg_sys::Oid,
        st_transform_geom_text_int: pg_sys::Oid,
        st_transform_geom_text_text: pg_sys::Oid,
        st_setsrid: pg_sys::Oid,
        geometry_oid: pg_sys::Oid,
    }

    #[derive(Clone, Copy)]
    struct CachedPostgisFns {
        database: pg_sys::Oid,
        generation: u64,
        fns: PostgisFns,
    }

    static CATALOG_GENERATION: AtomicU64 = AtomicU64::new(0);
    thread_local! {
        static POSTGIS_FNS: Cell<Option<CachedPostgisFns>> = const { Cell::new(None) };
    }

    unsafe extern "C-unwind" fn invalidate_postgis_fns(
        _arg: pg_sys::Datum,
        _cacheid: i32,
        _hashvalue: u32,
    ) {
        // Callbacks must not allocate, query catalogs, or borrow the cache.
        CATALOG_GENERATION.fetch_add(1, Ordering::Relaxed);
    }

    fn ensure_invalidation_callbacks() {
        static CALLBACKS: OnceLock<()> = OnceLock::new();
        CALLBACKS.get_or_init(|| unsafe {
            for cache in [
                pg_sys::SysCacheIdentifier::PROCOID,
                pg_sys::SysCacheIdentifier::TYPEOID,
                pg_sys::SysCacheIdentifier::NAMESPACEOID,
            ] {
                pg_sys::CacheRegisterSyscacheCallback(
                    cache as i32,
                    Some(invalidate_postgis_fns),
                    pg_sys::Datum::from(0usize),
                );
            }
        });
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

    fn extension_schema_oid(ext_oid: pg_sys::Oid) -> pg_sys::Oid {
        // Read pg_extension directly with AccessShareLock so lookups stay read-only
        // (safe inside parallel workers) and identical across PG15-19.
        unsafe {
            let lock = pg_sys::AccessShareLock as pg_sys::LOCKMODE;
            let rel = pg_sys::table_open(pg_sys::ExtensionRelationId, lock);
            let mut key = pg_sys::ScanKeyData::default();
            pg_sys::ScanKeyInit(
                &mut key,
                pg_sys::Anum_pg_extension_oid as pg_sys::AttrNumber,
                pg_sys::BTEqualStrategyNumber as pg_sys::StrategyNumber,
                pg_sys::Oid::from_u32(pg_sys::F_OIDEQ),
                pg_sys::Datum::from(ext_oid),
            );
            let scan = pg_sys::systable_beginscan(
                rel,
                pg_sys::Oid::from_u32(pg_sys::ExtensionOidIndexId),
                true,
                ptr::null_mut(),
                1,
                &mut key,
            );
            let tuple = pg_sys::systable_getnext(scan);
            let nsp_oid = if !tuple.is_null() {
                (*pg_sys::GETSTRUCT!(tuple)
                    .get()
                    .cast::<pg_sys::FormData_pg_extension>())
                .extnamespace
            } else {
                pg_sys::InvalidOid
            };
            pg_sys::systable_endscan(scan);
            pg_sys::table_close(rel, lock);
            nsp_oid
        }
    }

    fn postgis_schema_ident() -> String {
        unsafe {
            let ext_oid = pg_sys::get_extension_oid(c"postgis".as_ptr(), true);
            if ext_oid == pg_sys::InvalidOid {
                error!("PostGIS extension is not installed");
            }
            let nsp_oid = extension_schema_oid(ext_oid);
            if nsp_oid == pg_sys::InvalidOid {
                error!("PostGIS extension schema not found");
            }
            let nsp_name = pg_sys::get_namespace_name(nsp_oid);
            if nsp_name.is_null() {
                error!("PostGIS extension schema name not found");
            }
            let quoted = pg_sys::quote_identifier(nsp_name);
            let ident = CStr::from_ptr(quoted)
                .to_str()
                .unwrap_or_else(|err| error!("PostGIS schema name is not valid UTF-8: {err}"))
                .to_owned();
            if quoted != nsp_name.cast_const() {
                pg_sys::pfree(quoted as *mut std::ffi::c_void);
            }
            pg_sys::pfree(nsp_name.cast());
            ident
        }
    }

    fn postgis_fns() -> PostgisFns {
        ensure_invalidation_callbacks();
        let database = unsafe { pg_sys::MyDatabaseId };
        let generation = CATALOG_GENERATION.load(Ordering::Relaxed);
        if let Some(cached) = POSTGIS_FNS.with(Cell::get)
            && cached.database == database
            && cached.generation == generation
        {
            return cached.fns;
        }
        let fns = {
            // Resolve PostGIS objects in their owning schema, independent of search_path.
            let schema = postgis_schema_ident();
            let st_transform_geom_int = lookup_regprocedure_oid(&format!(
                "{schema}.st_transform({schema}.geometry,integer)"
            ));
            PostgisFns {
                st_transform_geom_int,
                st_transform_geom_text: lookup_regprocedure_oid(&format!(
                    "{schema}.st_transform({schema}.geometry,text)"
                )),
                st_transform_geom_text_int: lookup_regprocedure_oid(&format!(
                    "{schema}.st_transform({schema}.geometry,text,integer)"
                )),
                st_transform_geom_text_text: lookup_regprocedure_oid(&format!(
                    "{schema}.st_transform({schema}.geometry,text,text)"
                )),
                st_setsrid: lookup_regprocedure_oid(&format!(
                    "{schema}.st_setsrid({schema}.geometry,integer)"
                )),
                geometry_oid: unsafe { pg_sys::get_func_rettype(st_transform_geom_int) },
            }
        };
        POSTGIS_FNS.with(|cache| {
            cache.set(Some(CachedPostgisFns {
                database,
                generation,
                fns,
            }))
        });
        fns
    }

    #[derive(Clone, Copy)]
    enum PostgisFnSlot {
        TransformGeomInt = 0,
        TransformGeomText = 1,
        TransformGeomTextInt = 2,
        TransformGeomTextText = 3,
        SetSrid = 4,
    }

    #[repr(C)]
    struct CallSiteCache {
        database: pg_sys::Oid,
        generation: u64,
        fns: PostgisFns,
        slots: [pg_sys::FmgrInfo; 5],
    }

    #[repr(C)]
    struct LocalFcinfo3 {
        base: pg_sys::FunctionCallInfoBaseData,
        args: [pg_sys::NullableDatum; 3],
    }

    const _: () = {
        assert!(
            std::mem::offset_of!(LocalFcinfo3, args)
                == std::mem::offset_of!(pg_sys::FunctionCallInfoBaseData, args)
        );
    };

    struct CallSite {
        fns: PostgisFns,
        cache: *mut CallSiteCache,
        fn_mcxt: pg_sys::MemoryContext,
    }

    impl CallSite {
        fn from_fcinfo(fcinfo: pg_sys::FunctionCallInfo) -> Self {
            ensure_invalidation_callbacks();
            let database = unsafe { pg_sys::MyDatabaseId };
            let generation = CATALOG_GENERATION.load(Ordering::Relaxed);

            if !fcinfo.is_null() {
                let flinfo = unsafe { (*fcinfo).flinfo };
                if !flinfo.is_null() {
                    let fn_mcxt = unsafe { (*flinfo).fn_mcxt };
                    if !fn_mcxt.is_null() {
                        let mut cache = unsafe { (*flinfo).fn_extra.cast::<CallSiteCache>() };
                        if cache.is_null() {
                            let fns = postgis_fns();
                            let current_gen = CATALOG_GENERATION.load(Ordering::Relaxed);
                            unsafe {
                                cache = pg_sys::MemoryContextAllocZero(
                                    fn_mcxt,
                                    std::mem::size_of::<CallSiteCache>(),
                                )
                                .cast::<CallSiteCache>();
                                (*cache).database = database;
                                (*cache).generation = current_gen;
                                (*cache).fns = fns;
                                (*flinfo).fn_extra = cache.cast();
                            }
                            return Self {
                                fns,
                                cache,
                                fn_mcxt,
                            };
                        }

                        let (cached_db, cached_gen, cached_fns) = unsafe {
                            ((*cache).database, (*cache).generation, (*cache).fns)
                        };
                        if cached_db == database && cached_gen == generation {
                            return Self {
                                fns: cached_fns,
                                cache,
                                fn_mcxt,
                            };
                        }

                        let fns = postgis_fns();
                        let current_gen = CATALOG_GENERATION.load(Ordering::Relaxed);
                        unsafe {
                            (*cache).database = database;
                            (*cache).generation = current_gen;
                            (*cache).fns = fns;
                            (*cache).slots = [pg_sys::FmgrInfo::default(); 5];
                        }
                        return Self {
                            fns,
                            cache,
                            fn_mcxt,
                        };
                    }
                }
            }

            Self {
                fns: postgis_fns(),
                cache: ptr::null_mut(),
                fn_mcxt: ptr::null_mut(),
            }
        }

        #[inline]
        fn oid(&self, slot: PostgisFnSlot) -> pg_sys::Oid {
            match slot {
                PostgisFnSlot::TransformGeomInt => self.fns.st_transform_geom_int,
                PostgisFnSlot::TransformGeomText => self.fns.st_transform_geom_text,
                PostgisFnSlot::TransformGeomTextInt => self.fns.st_transform_geom_text_int,
                PostgisFnSlot::TransformGeomTextText => self.fns.st_transform_geom_text_text,
                PostgisFnSlot::SetSrid => self.fns.st_setsrid,
            }
        }

        fn call_nullable(
            &self,
            slot: PostgisFnSlot,
            args: &[pg_sys::Datum],
        ) -> Option<pg_sys::Datum> {
            // Keep per-call-site FmgrInfo in fn_mcxt so PostGIS retains PROJSRSCache
            // and SQL function plans across rows, while still handling NULL returns.
            assert!(args.len() <= 3);
            let oid = self.oid(slot);
            unsafe {
                let mut stack_flinfo = pg_sys::FmgrInfo::default();
                let flinfo = if !self.cache.is_null() {
                    let cached = &raw mut (*self.cache).slots[slot as usize];
                    if (*cached).fn_oid != oid || (*cached).fn_addr.is_none() {
                        *cached = pg_sys::FmgrInfo::default();
                        pg_sys::fmgr_info_cxt(oid, cached, self.fn_mcxt);
                    }
                    cached
                } else {
                    pg_sys::fmgr_info(oid, &mut stack_flinfo);
                    &raw mut stack_flinfo
                };

                let mut local = LocalFcinfo3 {
                    base: pg_sys::FunctionCallInfoBaseData::default(),
                    args: [pg_sys::NullableDatum::default(); 3],
                };
                local.base.flinfo = flinfo;
                local.base.fncollation = pg_sys::InvalidOid;
                local.base.nargs = args.len() as i16;
                for (arg_slot, value) in local.args.iter_mut().zip(args) {
                    arg_slot.value = *value;
                    arg_slot.isnull = false;
                }

                let func = (*flinfo)
                    .fn_addr
                    .unwrap_or_else(|| error!("PostGIS function has no address"));
                let fcinfo = (&raw mut local).cast::<pg_sys::FunctionCallInfoBaseData>();
                let datum = pg_sys::ffi::pg_guard_ffi_boundary(|| func(fcinfo));
                if (*fcinfo).isnull { None } else { Some(datum) }
            }
        }
    }

    /// Tracks ownership of a geometry varlena `Datum` so intermediate results are
    /// freed once without ever freeing or mutating a caller-borrowed datum.
    struct OwnedDatum {
        datum: pg_sys::Datum,
        owned: bool,
    }

    impl OwnedDatum {
        #[inline]
        fn borrowed(datum: pg_sys::Datum) -> Self {
            Self {
                datum,
                owned: false,
            }
        }

        #[inline]
        fn owned(datum: pg_sys::Datum) -> Self {
            Self { datum, owned: true }
        }

        #[inline]
        fn into_raw(mut self) -> pg_sys::Datum {
            self.owned = false;
            self.datum
        }
    }

    impl Drop for OwnedDatum {
        fn drop(&mut self) {
            if self.owned {
                unsafe { pg_sys::pfree(self.datum.cast_mut_ptr()) };
            }
        }
    }

    struct DetoastedGeometry {
        input: pg_sys::Datum,
        ptr: *mut pg_sys::varlena,
        owns_ptr: bool,
        input_owned: bool,
    }

    impl DetoastedGeometry {
        fn from_owned(input: OwnedDatum) -> Self {
            let input_owned = input.owned;
            let raw = input.into_raw();
            let original = raw.cast_mut_ptr::<pg_sys::varlena>();
            let ptr = unsafe { pg_sys::pg_detoast_datum(original) };
            if input_owned {
                if ptr != original {
                    unsafe { pg_sys::pfree(original.cast()) };
                }
                Self {
                    input: pg_sys::Datum::from(ptr),
                    ptr,
                    owns_ptr: true,
                    input_owned: true,
                }
            } else {
                Self {
                    input: raw,
                    ptr,
                    owns_ptr: ptr != original,
                    input_owned: false,
                }
            }
        }

        #[inline]
        fn len(&self) -> usize {
            unsafe { pg_sys::VARSIZE!(self.ptr).get() as usize }
        }

        fn srid(&self) -> i32 {
            let mut srid = 0i32;
            let rc = unsafe { pgct_gserialized_get_srid(self.ptr.cast::<u8>(), &mut srid) };
            if rc != 0 {
                error!("failed to read geometry SRID using librttopo, rc={rc}");
            }
            srid
        }

        fn make_writable(&mut self) {
            // librttopo's read/write iterator aliases serialized coordinate storage.
            // A detoast copy is already ours; an unmodified borrowed datum is not.
            if !self.owns_ptr {
                self.ptr = unsafe { pg_sys::pg_detoast_datum_copy(self.ptr) };
                self.owns_ptr = true;
            }
        }

        #[inline]
        fn into_owned_datum(mut self) -> OwnedDatum {
            debug_assert!(self.owns_ptr);
            self.owns_ptr = false;
            OwnedDatum::owned(pg_sys::Datum::from(self.ptr))
        }

        #[inline]
        fn into_identity(mut self) -> OwnedDatum {
            if self.input_owned {
                self.owns_ptr = false;
                OwnedDatum::owned(pg_sys::Datum::from(self.ptr))
            } else {
                OwnedDatum::borrowed(self.input)
            }
        }

        fn call_postgis_nullable(
            mut self,
            site: &CallSite,
            slot: PostgisFnSlot,
            extra_args: &[pg_sys::Datum],
        ) -> Option<OwnedDatum> {
            let arg = pg_sys::Datum::from(self.ptr);
            let mut args = [pg_sys::Datum::from(0usize); 3];
            args[0] = arg;
            for (idx, value) in extra_args.iter().enumerate() {
                args[idx + 1] = *value;
            }
            let out = site.call_nullable(slot, &args[..1 + extra_args.len()])?;
            if out == arg {
                let owned = self.owns_ptr;
                self.owns_ptr = false;
                Some(OwnedDatum { datum: out, owned })
            } else {
                Some(OwnedDatum::owned(out))
            }
        }

        #[inline]
        fn call_postgis_nonnull(
            self,
            site: &CallSite,
            slot: PostgisFnSlot,
            extra_args: &[pg_sys::Datum],
        ) -> OwnedDatum {
            self.call_postgis_nullable(site, slot, extra_args)
                .unwrap_or_else(|| error!("PostGIS function returned NULL"))
        }
    }

    impl Drop for DetoastedGeometry {
        fn drop(&mut self) {
            if self.owns_ptr {
                unsafe { pg_sys::pfree(self.ptr.cast()) };
            }
        }
    }

    fn call_postgis_nullable_consuming(
        mut input: OwnedDatum,
        site: &CallSite,
        slot: PostgisFnSlot,
        extra_args: &[pg_sys::Datum],
    ) -> Option<OwnedDatum> {
        let arg = input.datum;
        let mut args = [pg_sys::Datum::from(0usize); 3];
        args[0] = arg;
        for (idx, value) in extra_args.iter().enumerate() {
            args[idx + 1] = *value;
        }
        let out = site.call_nullable(slot, &args[..1 + extra_args.len()])?;
        if out == arg {
            let owned = input.owned;
            input.owned = false;
            Some(OwnedDatum { datum: out, owned })
        } else {
            Some(OwnedDatum::owned(out))
        }
    }

    #[inline]
    fn call_postgis_nonnull_consuming(
        input: OwnedDatum,
        site: &CallSite,
        slot: PostgisFnSlot,
        extra_args: &[pg_sys::Datum],
    ) -> OwnedDatum {
        call_postgis_nullable_consuming(input, site, slot, extra_args)
            .unwrap_or_else(|| error!("PostGIS function returned NULL"))
    }

    struct RttopoBuffer(*mut u8);

    impl Drop for RttopoBuffer {
        fn drop(&mut self) {
            unsafe { pgct_free(self.0.cast()) };
        }
    }

    fn apply_custom_mode(mut geom: DetoastedGeometry, mode: i32, dst_srid: i32) -> OwnedDatum {
        // Fast path for non-bbox Cartesian POINTs: mutate our owned detoast buffer
        // in place without librttopo deserialization, malloc/free, or an extra copy.
        let len = geom.len();
        if unsafe { pgct_can_transform_point_in_place(geom.ptr.cast::<u8>(), len) } != 0 {
            geom.make_writable();
            let rc = unsafe {
                pgct_transform_point_in_place(geom.ptr.cast::<u8>(), len, mode, dst_srid)
            };
            if rc != 0 {
                error!("point transform failed, rc={rc}");
            }
            return geom.into_owned_datum();
        }

        geom.make_writable();
        let mut out_ptr: *mut u8 = ptr::null_mut();
        let mut out_len = 0usize;
        let rc = unsafe {
            pgct_transform_gserialized(
                geom.ptr.cast::<u8>(),
                mode,
                dst_srid,
                &mut out_ptr,
                &mut out_len,
            )
        };

        let buffer = RttopoBuffer(out_ptr);
        if rc != 0 || out_ptr.is_null() || out_len == 0 {
            error!("librttopo transform failed, rc={rc}, len={out_len}");
        }
        if out_len > i32::MAX as usize {
            error!("librttopo transform output too large: {out_len}");
        }

        let out_varlena = unsafe { pg_sys::palloc(out_len) as *mut u8 };
        if out_varlena.is_null() {
            error!("palloc failed for transformed geometry, len={out_len}");
        }

        unsafe {
            ptr::copy_nonoverlapping(buffer.0, out_varlena, out_len);
            pg_sys::SET_VARSIZE!(out_varlena.cast::<pg_sys::varlena>(), out_len as i32);
        }

        OwnedDatum::owned(pg_sys::Datum::from(out_varlena.cast::<pg_sys::varlena>()))
    }

    #[pg_extern(immutable, strict, parallel_safe, name = "st_eviltransform")]
    fn st_eviltransform_integer(
        geom: AnyElement,
        dst_srid: i32,
        fcinfo: pg_sys::FunctionCallInfo,
    ) -> AnyElement {
        let site = CallSite::from_fcinfo(fcinfo);
        // anyelement also accepts scalars; only geometry storage can be detoasted here.
        if geom.oid() != site.fns.geometry_oid
            && unsafe { pg_sys::getBaseType(geom.oid()) } != site.fns.geometry_oid
        {
            error!("ST_EvilTransform requires a PostGIS geometry");
        }
        let result = transform_integer(OwnedDatum::borrowed(geom.datum()), dst_srid, &site);
        geometry_result(result.into_raw(), geom.oid())
    }

    fn transform_integer(input: OwnedDatum, dst_srid: i32, site: &CallSite) -> OwnedDatum {
        let geometry = DetoastedGeometry::from_owned(input);
        let src_srid = geometry.srid();

        if src_srid == dst_srid {
            geometry.into_identity()
        } else if src_srid != SRID_GCJ02
            && src_srid != SRID_BD09
            && dst_srid != SRID_GCJ02
            && dst_srid != SRID_BD09
        {
            geometry.call_postgis_nonnull(site, PostgisFnSlot::TransformGeomInt, &[dst_srid.into()])
        } else if src_srid == SRID_GCJ02 && dst_srid == SRID_BD09 {
            apply_custom_mode(geometry, MODE_GCJ2BD, SRID_BD09)
        } else if src_srid == SRID_BD09 && dst_srid == SRID_GCJ02 {
            apply_custom_mode(geometry, MODE_BD2GCJ, SRID_GCJ02)
        } else if src_srid == SRID_GCJ02 {
            let wgs = apply_custom_mode(geometry, MODE_GCJ2WGS, SRID_WGS84);
            if dst_srid == SRID_WGS84 {
                wgs
            } else {
                call_postgis_nonnull_consuming(
                    wgs,
                    site,
                    PostgisFnSlot::TransformGeomInt,
                    &[dst_srid.into()],
                )
            }
        } else if src_srid == SRID_BD09 {
            let wgs = apply_custom_mode(geometry, MODE_BD2WGS, SRID_WGS84);
            if dst_srid == SRID_WGS84 {
                wgs
            } else {
                call_postgis_nonnull_consuming(
                    wgs,
                    site,
                    PostgisFnSlot::TransformGeomInt,
                    &[dst_srid.into()],
                )
            }
        } else if dst_srid == SRID_GCJ02 {
            let wgs = if src_srid == SRID_WGS84 {
                geometry
            } else {
                DetoastedGeometry::from_owned(geometry.call_postgis_nonnull(
                    site,
                    PostgisFnSlot::TransformGeomInt,
                    &[SRID_WGS84.into()],
                ))
            };
            apply_custom_mode(wgs, MODE_WGS2GCJ, SRID_GCJ02)
        } else if dst_srid == SRID_BD09 {
            let wgs = if src_srid == SRID_WGS84 {
                geometry
            } else {
                DetoastedGeometry::from_owned(geometry.call_postgis_nonnull(
                    site,
                    PostgisFnSlot::TransformGeomInt,
                    &[SRID_WGS84.into()],
                ))
            };
            apply_custom_mode(wgs, MODE_WGS2BD, SRID_BD09)
        } else {
            geometry.call_postgis_nonnull(site, PostgisFnSlot::TransformGeomInt, &[dst_srid.into()])
        }
    }

    fn geometry_result(result: pg_sys::Datum, oid: pg_sys::Oid) -> AnyElement {
        unsafe { <AnyElement as FromDatum>::from_polymorphic_datum(result, false, oid) }
            .unwrap_or_else(|| error!("failed to build transformed geometry datum"))
    }

    fn custom_srid(spec: &str) -> Option<i32> {
        let trimmed = spec.trim();
        if trimmed.eq_ignore_ascii_case("990001")
            || trimmed.eq_ignore_ascii_case("EPSG:990001")
            || trimmed.eq_ignore_ascii_case("GCJ02")
            || trimmed.eq_ignore_ascii_case("GCJ-02")
        {
            Some(SRID_GCJ02)
        } else if trimmed.eq_ignore_ascii_case("990002")
            || trimmed.eq_ignore_ascii_case("EPSG:990002")
            || trimmed.eq_ignore_ascii_case("BD09")
            || trimmed.eq_ignore_ascii_case("BD-09")
        {
            Some(SRID_BD09)
        } else if trimmed.eq_ignore_ascii_case("4490")
            || trimmed.eq_ignore_ascii_case("EPSG:4490")
            || trimmed.eq_ignore_ascii_case("CGCS2000")
            || trimmed.eq_ignore_ascii_case("CGCS-2000")
        {
            Some(4490)
        } else {
            None
        }
    }

    fn text_arg_datum(fcinfo: pg_sys::FunctionCallInfo, index: usize, value: &str) -> OwnedDatum {
        unsafe {
            if !fcinfo.is_null() && ((*fcinfo).nargs as usize) > index {
                return OwnedDatum::borrowed((*fcinfo).args.as_slice(index + 1)[index].value);
            }
        }
        OwnedDatum::owned(value.into_datum().unwrap())
    }

    fn transform_text(
        input: OwnedDatum,
        to_proj: &str,
        to_proj_datum: pg_sys::Datum,
        site: &CallSite,
    ) -> Option<OwnedDatum> {
        if let Some(dst) = custom_srid(to_proj) {
            return Some(transform_integer(input, dst, site));
        }
        let geometry = DetoastedGeometry::from_owned(input);
        let source = geometry.srid();
        match source {
            SRID_GCJ02 => {
                let wgs = apply_custom_mode(geometry, MODE_GCJ2WGS, SRID_WGS84);
                call_postgis_nullable_consuming(
                    wgs,
                    site,
                    PostgisFnSlot::TransformGeomText,
                    &[to_proj_datum],
                )
            }
            SRID_BD09 => {
                let wgs = apply_custom_mode(geometry, MODE_BD2WGS, SRID_WGS84);
                call_postgis_nullable_consuming(
                    wgs,
                    site,
                    PostgisFnSlot::TransformGeomText,
                    &[to_proj_datum],
                )
            }
            _ => geometry.call_postgis_nullable(
                site,
                PostgisFnSlot::TransformGeomText,
                &[to_proj_datum],
            ),
        }
    }

    fn transform_from_text_integer(
        input: OwnedDatum,
        from_proj: &str,
        from_proj_datum: pg_sys::Datum,
        to_srid: i32,
        site: &CallSite,
    ) -> Option<OwnedDatum> {
        if let Some(src) = custom_srid(from_proj) {
            let relabeled =
                call_postgis_nonnull_consuming(input, site, PostgisFnSlot::SetSrid, &[src.into()]);
            Some(transform_integer(relabeled, to_srid, site))
        } else if matches!(to_srid, SRID_GCJ02 | SRID_BD09) {
            let wgs = call_postgis_nullable_consuming(
                input,
                site,
                PostgisFnSlot::TransformGeomTextInt,
                &[from_proj_datum, SRID_WGS84.into()],
            )?;
            Some(transform_integer(wgs, to_srid, site))
        } else {
            call_postgis_nullable_consuming(
                input,
                site,
                PostgisFnSlot::TransformGeomTextInt,
                &[from_proj_datum, to_srid.into()],
            )
        }
    }

    #[pg_extern(sql = r#"
        CREATE FUNCTION st_eviltransform(geom geometry, to_proj text) RETURNS geometry
        LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
        AS 'MODULE_PATHNAME', '@FUNCTION_NAME@';
    "#)]
    fn st_eviltransform_text(
        geom: AnyElement,
        to_proj: &str,
        fcinfo: pg_sys::FunctionCallInfo,
    ) -> Option<AnyElement> {
        let site = CallSite::from_fcinfo(fcinfo);
        let to_proj_datum = text_arg_datum(fcinfo, 1, to_proj);
        transform_text(
            OwnedDatum::borrowed(geom.datum()),
            to_proj,
            to_proj_datum.datum,
            &site,
        )
        .map(|result| geometry_result(result.into_raw(), site.fns.geometry_oid))
    }

    #[pg_extern(sql = r#"
        CREATE FUNCTION st_eviltransform(geom geometry, from_proj text, to_srid integer) RETURNS geometry
        LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
        AS 'MODULE_PATHNAME', '@FUNCTION_NAME@';
    "#)]
    fn st_eviltransform_from_text_integer(
        geom: AnyElement,
        from_proj: &str,
        to_srid: i32,
        fcinfo: pg_sys::FunctionCallInfo,
    ) -> Option<AnyElement> {
        let site = CallSite::from_fcinfo(fcinfo);
        let from_proj_datum = text_arg_datum(fcinfo, 1, from_proj);
        transform_from_text_integer(
            OwnedDatum::borrowed(geom.datum()),
            from_proj,
            from_proj_datum.datum,
            to_srid,
            &site,
        )
        .map(|result| geometry_result(result.into_raw(), site.fns.geometry_oid))
    }

    #[pg_extern(sql = r#"
        CREATE FUNCTION st_eviltransform(geom geometry, from_proj text, to_proj text) RETURNS geometry
        LANGUAGE c IMMUTABLE STRICT PARALLEL SAFE
        AS 'MODULE_PATHNAME', '@FUNCTION_NAME@';
    "#)]
    fn st_eviltransform_from_text(
        geom: AnyElement,
        from_proj: &str,
        to_proj: &str,
        fcinfo: pg_sys::FunctionCallInfo,
    ) -> Option<AnyElement> {
        let site = CallSite::from_fcinfo(fcinfo);
        let from_proj_datum = text_arg_datum(fcinfo, 1, from_proj);
        let input = OwnedDatum::borrowed(geom.datum());
        let result = if let Some(dst) = custom_srid(to_proj) {
            transform_from_text_integer(input, from_proj, from_proj_datum.datum, dst, &site)
        } else if let Some(src) = custom_srid(from_proj) {
            let to_proj_datum = text_arg_datum(fcinfo, 2, to_proj);
            let relabeled =
                call_postgis_nonnull_consuming(input, &site, PostgisFnSlot::SetSrid, &[src.into()]);
            transform_text(relabeled, to_proj, to_proj_datum.datum, &site)
        } else {
            let to_proj_datum = text_arg_datum(fcinfo, 2, to_proj);
            call_postgis_nullable_consuming(
                input,
                &site,
                PostgisFnSlot::TransformGeomTextText,
                &[from_proj_datum.datum, to_proj_datum.datum],
            )
        };
        result.map(|result| geometry_result(result.into_raw(), site.fns.geometry_oid))
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

    fn collect_array_values<T, I, F>(values: I, convert: F) -> impl Iterator<Item = f64>
    where
        I: IntoIterator<Item = Option<T>>,
        F: Fn(T) -> f64,
    {
        values.into_iter().flatten().map(convert)
    }

    macro_rules! scalar_array_jenks {
        ($values:expr, $breaks:expr, $invert:expr, $convert:expr) => {{
            let convert = $convert;
            if let Ok(slice) = $values.as_slice() {
                values_to_jenks(slice.iter().copied().map(convert), $breaks, $invert)
            } else {
                values_to_jenks(collect_array_values($values, convert), $breaks, $invert)
            }
        }};
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float8_array(values: Array<'_, f64>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_float8_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float8_array_invert(
        values: Array<'_, f64>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        scalar_array_jenks!(values, breaks, invert, |value| value)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float4_array(values: Array<'_, f32>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_float4_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_float4_array_invert(
        values: Array<'_, f32>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        scalar_array_jenks!(values, breaks, invert, f64::from)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int8_array(values: Array<'_, i64>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int8_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int8_array_invert(
        values: Array<'_, i64>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        scalar_array_jenks!(values, breaks, invert, |value| value as f64)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int4_array(values: Array<'_, i32>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int4_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int4_array_invert(
        values: Array<'_, i32>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        scalar_array_jenks!(values, breaks, invert, f64::from)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int2_array(values: Array<'_, i16>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_int2_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_int2_array_invert(
        values: Array<'_, i16>,
        breaks: i32,
        invert: bool,
    ) -> Option<Vec<f64>> {
        scalar_array_jenks!(values, breaks, invert, f64::from)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_numeric_array(values: Array<'_, AnyNumeric>, breaks: i32) -> Option<Vec<f64>> {
        st_jenksbins_numeric_array_invert(values, breaks, false)
    }

    #[pg_extern(immutable, parallel_safe, name = "st_jenksbins")]
    fn st_jenksbins_numeric_array_invert(
        values: Array<'_, AnyNumeric>,
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
        fn add_value(
            &mut self,
            value: Option<f64>,
            breaks: Option<i32>,
            invert: Option<bool>,
        ) {
            let breaks = breaks.unwrap_or_else(|| error!("breaks must not be NULL"));
            let invert =
                invert.unwrap_or_else(|| error!("ST_JenksBins invert argument must not be NULL"));
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

        fn finalize(&self) -> Option<Vec<f64>> {
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
        type Args = (Option<f64>, Option<i32>);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value, breaks, Some(false));
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbF8Inv;

    #[pg_aggregate]
    impl Aggregate<JbF8Inv> for JbF8Inv {
        type Args = (Option<f64>, Option<i32>, Option<bool>);
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
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbNum;

    #[pg_aggregate]
    impl Aggregate<JbNum> for JbNum {
        type Args = (Option<AnyNumeric>, Option<i32>);
        type State = Internal;
        type Finalize = Option<Vec<f64>>;

        fn state(
            mut current: Self::State,
            (value, breaks): Self::Args,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::State {
            let state = unsafe { current.get_or_insert_default::<JenksBinsState>() };
            state.add_value(value.map(anynumeric_to_f64), breaks, Some(false));
            current
        }

        fn finalize(
            current: Self::State,
            _direct_args: Self::OrderedSetArgs,
            _fcinfo: pg_sys::FunctionCallInfo,
        ) -> Self::Finalize {
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.finalize())
        }
    }

    #[derive(AggregateName)]
    #[aggregate_name = "st_jenksbins"]
    struct JbNumInv;

    #[pg_aggregate]
    impl Aggregate<JbNumInv> for JbNumInv {
        type Args = (Option<AnyNumeric>, Option<i32>, Option<bool>);
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
            unsafe { current.get::<JenksBinsState>() }.and_then(|state| state.finalize())
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
          WHEN '4490' THEN 4490
          WHEN 'EPSG:4490' THEN 4490
          WHEN 'CGCS2000' THEN 4490
          WHEN 'CGCS-2000' THEN 4490
          ELSE NULL
        END;
        $$;

        "#,
        name = "st_eviltransform_sql",
        requires = [st_eviltransform_integer, st_eviltransform_ewkb_internal]
    );

    #[cfg(any(test, feature = "pg_test"))]
    #[pg_schema]
    mod tests {
        use pgrx::prelude::*;

        #[pg_test(error = "ST_EvilTransform requires a PostGIS geometry")]
        fn test_rejects_non_geometry_before_pointer_access() {
            Spi::run("SELECT ST_EvilTransform(42, 990001)").unwrap();
        }

        #[pg_test]
        fn test_geometry_domain_preserves_type_and_coordinates() {
            Spi::run("CREATE DOMAIN test_geometry AS geometry").unwrap();
            assert_eq!(Spi::get_one::<bool>(
                "SELECT pg_typeof(ST_EvilTransform(g, 990001)) = 'test_geometry'::regtype AND ST_AsEWKB(ST_EvilTransform(g, 990001)) = ST_AsEWKB(ST_EvilTransform(g::geometry, 990001)) FROM (SELECT 'SRID=4326;POINT(120 30)'::test_geometry AS g) t"
            ).unwrap(), Some(true));
        }

        #[pg_test]
        fn test_integer_overload_to_gcj02() {
            // The legacy reference uses a different earth radius; compare coordinates,
            // at the same tolerance as the core reference tests, rather than WKT text.
            let got = Spi::get_one::<bool>(
                "SELECT ST_SRID(g) = 990001 AND abs(ST_X(g) - 120.004660445597) < 1e-6 AND abs(ST_Y(g) - 29.997534331696) < 1e-6 FROM (SELECT ST_EvilTransform('SRID=4326;POINT(120 30)'::geometry, 990001) AS g) t",
            )
            .expect("SPI failed")
            .expect("no row returned");

            assert!(got);
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
        fn test_cgcs2000_integer_delegates_to_postgis() {
            for sql in [
                "SELECT ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 4490)) = ST_AsEWKB(ST_Transform(ST_SetSRID('POINT(120 30)'::geometry, 4326), 4490))",
                "SELECT ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4490), 4326)) = ST_AsEWKB(ST_Transform(ST_SetSRID('POINT(120 30)'::geometry, 4490), 4326))",
            ] {
                let got = Spi::get_one::<bool>(sql)
                    .expect("SPI failed")
                    .expect("no row returned");
                assert!(got, "{sql}");
            }
        }

        #[pg_test]
        fn test_cgcs2000_destination_aliases() {
            for alias in ["CGCS2000", "CGCS-2000", "4490", "EPSG:4490"] {
                let sql = format!(
                    "SELECT ST_SRID(ST_EvilTransform(g, '{alias}')) = 4490 AND ST_AsEWKB(ST_EvilTransform(g, '{alias}')) = ST_AsEWKB(ST_EvilTransform(g, 4490)) FROM (SELECT ST_SetSRID('POINT(120 30)'::geometry, 4326) AS g) AS t"
                );
                let got = Spi::get_one::<bool>(&sql)
                    .expect("SPI failed")
                    .expect("no row returned");
                assert!(got, "{sql}");
            }
        }

        #[pg_test]
        fn test_cgcs2000_named_source_and_custom_crs() {
            for sql in [
                "SELECT ST_AsEWKB(ST_EvilTransform('POINT(120 30)'::geometry, 'CGCS2000', 4326)) = ST_AsEWKB(ST_Transform(ST_SetSRID('POINT(120 30)'::geometry, 4490), 4326))",
                "SELECT ST_AsEWKB(ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4490', 'GCJ02')) = ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4490), 990001))",
                "SELECT ST_AsEWKB(ST_EvilTransform('POINT(120 30)'::geometry, 'CGCS-2000', 990002)) = ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 4490), 990002))",
                "SELECT ST_AsEWKB(ST_EvilTransform('POINT(120 30)'::geometry, 'GCJ02', 'CGCS2000')) = ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 990001), 4490))",
                "SELECT ST_AsEWKB(ST_EvilTransform('POINT(120 30)'::geometry, 'BD09', 'EPSG:4490')) = ST_AsEWKB(ST_EvilTransform(ST_SetSRID('POINT(120 30)'::geometry, 990002), 4490))",
            ] {
                let got = Spi::get_one::<bool>(sql)
                    .expect("SPI failed")
                    .expect("no row returned");
                assert!(got, "{sql}");
            }
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
            Spi::run(
                "DO $$
                DECLARE source geometry := 'SRID=4326;POINT(120 30)'::geometry;
                        first geometry;
                        second geometry;
                BEGIN
                  first := ST_EvilTransform(source, 990001);
                  second := ST_EvilTransform(source, 990001);
                  IF ST_X(source) <> 120 OR ST_Y(source) <> 30
                    OR abs(ST_X(first) - 120.004660445597) > 1e-6
                    OR abs(ST_X(second) - 120.004660445597) > 1e-6 THEN
                    RAISE EXCEPTION 'custom transform mutated borrowed input';
                  END IF;
                END $$",
            )
            .unwrap();
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
        fn test_transformed_bbox_and_ordinates() {
            Spi::run(
                "DO $$
                DECLARE wkt text; source geometry; output geometry; pair record;
                BEGIN
                  FOREACH wkt IN ARRAY ARRAY[
                    'POINT(120 30)', 'POINT Z(120 30 7)',
                    'POINT M(120 30 8)', 'POINT ZM(120 30 7 8)',
                    'LINESTRING(120 30,121 31)',
                    'LINESTRING Z(120 30 7,121 31 9)',
                    'LINESTRING M(120 30 8,121 31 10)',
                    'LINESTRING ZM(120 30 7 8,121 31 9 10)',
                    'POINT EMPTY', 'LINESTRING EMPTY',
                    'MULTILINESTRING((120 30,121 31),(122 32,123 33))',
                    'GEOMETRYCOLLECTION(POINT(120 30),LINESTRING(121 31,122 32))'
                  ] LOOP
                    source := ST_GeomFromText(wkt, 4326);
                    output := ST_EvilTransform(source, 990001);
                    IF ST_GeometryType(output) <> ST_GeometryType(source)
                       OR ST_Zmflag(output) <> ST_Zmflag(source)
                       OR ST_NPoints(output) <> ST_NPoints(source)
                       OR ST_IsEmpty(output) <> ST_IsEmpty(source)
                       OR ST_SRID(output) <> 990001 THEN
                      RAISE EXCEPTION 'geometry structure changed: %', wkt;
                    END IF;
                    FOR pair IN
                      SELECT a.geom original, b.geom transformed
                      FROM ST_DumpPoints(source) a JOIN ST_DumpPoints(output) b USING (path)
                    LOOP
                      IF ST_Z(pair.original) IS DISTINCT FROM ST_Z(pair.transformed)
                         OR ST_M(pair.original) IS DISTINCT FROM ST_M(pair.transformed)
                         OR NOT (output && pair.transformed)
                         OR NOT (output &&& pair.transformed) THEN
                        RAISE EXCEPTION 'ordinates or cached bounds changed: %', wkt;
                      END IF;
                    END LOOP;
                  END LOOP;
                END $$",
            )
            .unwrap();
            assert_eq!(Spi::get_one::<bool>(
                "WITH transformed AS (SELECT ST_EvilTransform('SRID=4326;LINESTRING ZM (120 30 7 8,121 31 9 10)'::geometry, 990001) AS g)
                 SELECT ST_Covers(ST_Envelope(g), ST_Force2D(g))
                   AND ST_XMin(g::box3d) > 120.004
                   AND ST_Z(ST_StartPoint(g)) = 7 AND ST_M(ST_EndPoint(g)) = 10
                   AND ST_SRID(g) = 990001 FROM transformed"
            ).unwrap(), Some(true));
        }

        #[pg_test]
        fn test_explicit_source_overrides_geometry_srid() {
            for sql in [
                "SELECT abs(ST_X(g) - 120.004660445597) < 1e-6 AND ST_SRID(g) = 990001 FROM (SELECT ST_EvilTransform('SRID=3857;POINT(120 30)'::geometry, 'EPSG:4326', 990001) AS g) t",
                "SELECT ST_X(g) = 120 AND ST_Y(g) = 30 AND ST_SRID(g) = 990001 FROM (SELECT ST_EvilTransform('SRID=3857;POINT(120 30)'::geometry, 'GCJ02', 'GCJ02') AS g) t",
                "SELECT ST_IsEmpty(ST_EvilTransform('SRID=4326;GEOMETRYCOLLECTION EMPTY'::geometry, 'BD09'))",
                "SELECT ST_EvilTransform('POINT(120 30)'::geometry, '+proj=longlat +datum=WGS84 +no_defs') IS NULL",
                "SELECT ST_EvilTransform('POINT(120 30)'::geometry, 'EPSG:4326', 0) IS NULL",
                "SELECT ST_EvilTransform(NULL::geometry, 'GCJ02') IS NULL",
            ] {
                assert_eq!(Spi::get_one::<bool>(sql).unwrap(), Some(true), "{sql}");
            }
        }

        #[pg_test]
        fn test_toasted_geometry_and_identity_lifetime() {
            Spi::run("CREATE TEMP TABLE toasted_geometry(g geometry);
                ALTER TABLE toasted_geometry ALTER COLUMN g SET STORAGE EXTERNAL;
                INSERT INTO toasted_geometry SELECT ST_SetSRID(ST_MakeLine(ST_MakePoint(120 + i * 0.000001, 30 + i * 0.000001)), 4326) FROM generate_series(1, 5000) i").unwrap();
            assert_eq!(
                Spi::get_one::<bool>(
                    "SELECT pg_column_size(g) > 8000
                   AND ST_NPoints(ST_EvilTransform(g, 990001)) = 5000
                   AND ST_AsEWKB(ST_EvilTransform(g, 4326)) = ST_AsEWKB(g)
                   AND ST_X(ST_StartPoint(g)) = 120.000001
                 FROM toasted_geometry"
                )
                .unwrap(),
                Some(true)
            );
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
        fn test_jenksbins_array_nulls_and_inverted_edges() {
            for ty in [
                "double precision",
                "real",
                "bigint",
                "integer",
                "smallint",
                "numeric",
            ] {
                let sql = format!(
                    "SELECT ST_JenksBins(NULL::{ty}[], 2) IS NULL
                       AND ST_JenksBins(ARRAY[]::{ty}[], 2) IS NULL
                       AND ST_JenksBins(ARRAY[NULL, NULL]::{ty}[], 2) IS NULL
                       AND ST_JenksBins(ARRAY[1, 1, NULL, 2, 50, 51]::{ty}[], 2, true) = ARRAY[1,50]::double precision[]"
                );
                assert_eq!(Spi::get_one::<bool>(&sql).unwrap(), Some(true), "{sql}");
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

        #[pg_test]
        fn test_parallel_query_all_overloads() {
            Spi::run(
                "DO $$
                DECLARE
                  s1 double precision;
                  s2 double precision;
                  s3 double precision;
                  s4 double precision;
                  s5 double precision;
                BEGIN
                  IF current_setting('server_version_num')::int >= 160000 THEN
                    PERFORM set_config('debug_parallel_query', 'on', true);
                  ELSE
                    PERFORM set_config('force_parallel_mode', 'on', true);
                  END IF;
                  CREATE TEMP TABLE par_pts AS
                    SELECT ST_SetSRID(ST_MakePoint(120 + g * 0.0001, 30 + g * 0.0001), 4326) AS g
                    FROM generate_series(1, 200) AS g;
                  SELECT sum(ST_X(ST_EvilTransform(g, 990001))) INTO s1 FROM par_pts;
                  SELECT sum(ST_X(ST_EvilTransform(g, 'BD09'))) INTO s2 FROM par_pts;
                  SELECT sum(ST_X(ST_EvilTransform(g, 'EPSG:4326', 990001))) INTO s3 FROM par_pts;
                  SELECT sum(ST_X(ST_EvilTransform(g, 'EPSG:4326', 'BD09'))) INTO s4 FROM par_pts;
                  SELECT sum(ST_X(ST_EvilTransform(ST_EvilTransform(g, 990002), 3857))) INTO s5 FROM par_pts;
                  IF abs(s1 - s3) > 1e-6 OR abs(s2 - s4) > 1e-6 OR s5 IS NULL OR s5 <= 0 THEN
                    RAISE EXCEPTION 'parallel query mismatch: %, %, %, %, %', s1, s2, s3, s4, s5;
                  END IF;
                END $$",
            )
            .unwrap();
        }

        #[pg_test]
        fn test_polygon_collection_curve_and_point_bbox_transforms() {
            Spi::run(
                "DO $$
                DECLARE wkt text; source geometry; output geometry; pair record;
                BEGIN
                  FOREACH wkt IN ARRAY ARRAY[
                    'POLYGON((120 30,121 30,121 31,120 31,120 30),(120.2 30.2,120.8 30.2,120.8 30.8,120.2 30.8,120.2 30.2))',
                    'MULTIPOLYGON(((120 30,121 30,121 31,120 30)),((121.5 31.5,122 31.5,122 32,121.5 31.5)))',
                    'MULTIPOINT Z ((120 30 5),(121 31 6))',
                    'CIRCULARSTRING(120 30,120.5 30.5,121 30)',
                    'CURVEPOLYGON(CIRCULARSTRING(120 30,120.5 30.5,121 30,120.5 29.5,120 30))',
                    'TRIANGLE((120 30,121 30,120.5 31,120 30))',
                    'POLYHEDRALSURFACE Z (((120 30 0,121 30 0,121 31 1,120 31 1,120 30 0)))',
                    'TIN Z (((120 30 0,121 30 0,120.5 31 1,120 30 0)))'
                  ] LOOP
                    source := PostGIS_AddBBox(ST_GeomFromText(wkt, 4326));
                    output := ST_EvilTransform(source, 990001);
                    IF ST_GeometryType(output) <> ST_GeometryType(source)
                       OR ST_Zmflag(output) <> ST_Zmflag(source)
                       OR ST_NPoints(output) <> ST_NPoints(source)
                       OR ST_SRID(output) <> 990001 THEN
                      RAISE EXCEPTION 'geometry structure changed: %', wkt;
                    END IF;
                    FOR pair IN
                      SELECT a.geom original, b.geom transformed
                      FROM ST_DumpPoints(source) a JOIN ST_DumpPoints(output) b USING (path)
                    LOOP
                      IF ST_Z(pair.original) IS DISTINCT FROM ST_Z(pair.transformed)
                         OR NOT (output && pair.transformed)
                         OR NOT (output &&& pair.transformed) THEN
                        RAISE EXCEPTION 'ordinates or cached bounds changed: %', wkt;
                      END IF;
                    END LOOP;
                  END LOOP;
                  source := PostGIS_AddBBox('SRID=4326;POINT(120 30)'::geometry);
                  output := ST_EvilTransform(source, 990001);
                  IF NOT (output && ST_MakePoint(ST_X(output), ST_Y(output)))
                     OR ST_SRID(output) <> 990001 THEN
                    RAISE EXCEPTION 'point with cached bbox produced stale bounds';
                  END IF;
                END $$",
            )
            .unwrap();
        }

        #[pg_test]
        fn test_jenksbins_numerical_stability_and_nonnull_scalar_arrays() {
            for sql in [
                "SELECT ST_JenksBins(ARRAY[1000000001,1000000001,1000000002,1000000003,1000000010,1000000011,1000000012]::double precision[], 2) = ARRAY[1000000003,1000000012]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1e200,1e200,2e200,3e200,1e201,1.1e201,1.2e201]::double precision[], 2) = ARRAY[3e200,1.2e201]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1e-200,1e-200,2e-200,3e-200,1e-199,1.1e-199,1.2e-199]::double precision[], 2) = ARRAY[3e-200,1.2e-199]::double precision[]",
                "SELECT ST_JenksBins(v, 2) = ARRAY[1000000003,1000000012]::double precision[] FROM unnest(ARRAY[1000000001,1000000001,1000000002,1000000003,1000000010,1000000011,1000000012]::double precision[]) v",
                "SELECT ST_JenksBins(ARRAY[1,2,3,10,11,12]::double precision[], 1) = ARRAY[12]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1,2,3,10,11,12]::double precision[], 1, true) = ARRAY[1]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1,2,10,11]::real[], 2) = ARRAY[2,11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1,2,10,11]::bigint[], 2) = ARRAY[2,11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1,2,10,11]::integer[], 2) = ARRAY[2,11]::double precision[]",
                "SELECT ST_JenksBins(ARRAY[1,2,10,11]::smallint[], 2) = ARRAY[2,11]::double precision[]",
            ] {
                assert_eq!(Spi::get_one::<bool>(sql).unwrap(), Some(true), "{sql}");
            }
        }

        #[pg_test(error = "breaks must not be NULL")]
        fn test_jenksbins_aggregate_rejects_null_breaks() {
            Spi::run("SELECT ST_JenksBins(1.0::double precision, NULL::integer)").unwrap();
        }

        #[pg_test(error = "ST_JenksBins invert argument must not be NULL")]
        fn test_jenksbins_aggregate_rejects_null_invert() {
            Spi::run("SELECT ST_JenksBins(1.0::double precision, 2, NULL::boolean)").unwrap();
        }
    }
}
