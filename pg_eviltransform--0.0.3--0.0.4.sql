/* <begin connected objects> */
-- src/lib.rs:445
-- pg_eviltransform::extension::jb_f_8_inv_jb_f_8_inv_finalize
CREATE  FUNCTION "jb_f_8_inv_jb_f_8_inv_finalize"(
	"this" internal /* Internal */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_f_8_inv_jb_f_8_inv_finalize_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:445
-- pg_eviltransform::extension::jb_f_8_inv_jb_f_8_inv_state
CREATE  FUNCTION "jb_f_8_inv_jb_f_8_inv_state"(
	"this" internal, /* Internal */
	"arg_one" double precision, /* Option < f64 > */
	"arg_two" INT, /* i32 */
	"arg_three" bool /* bool */
) RETURNS internal /* Internal */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_f_8_inv_jb_f_8_inv_state_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:416
-- pg_eviltransform::extension::jb_f_8_jb_f_8_finalize
CREATE  FUNCTION "jb_f_8_jb_f_8_finalize"(
	"this" internal /* Internal */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_f_8_jb_f_8_finalize_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:416
-- pg_eviltransform::extension::jb_f_8_jb_f_8_state
CREATE  FUNCTION "jb_f_8_jb_f_8_state"(
	"this" internal, /* Internal */
	"arg_one" double precision, /* Option < f64 > */
	"arg_two" INT /* i32 */
) RETURNS internal /* Internal */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_f_8_jb_f_8_state_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:503
-- pg_eviltransform::extension::jb_num_inv_jb_num_inv_finalize
CREATE  FUNCTION "jb_num_inv_jb_num_inv_finalize"(
	"this" internal /* Internal */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_num_inv_jb_num_inv_finalize_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:503
-- pg_eviltransform::extension::jb_num_inv_jb_num_inv_state
CREATE  FUNCTION "jb_num_inv_jb_num_inv_state"(
	"this" internal, /* Internal */
	"arg_one" NUMERIC, /* Option < AnyNumeric > */
	"arg_two" INT, /* i32 */
	"arg_three" bool /* bool */
) RETURNS internal /* Internal */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_num_inv_jb_num_inv_state_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:474
-- pg_eviltransform::extension::jb_num_jb_num_finalize
CREATE  FUNCTION "jb_num_jb_num_finalize"(
	"this" internal /* Internal */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_num_jb_num_finalize_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:474
-- pg_eviltransform::extension::jb_num_jb_num_state
CREATE  FUNCTION "jb_num_jb_num_state"(
	"this" internal, /* Internal */
	"arg_one" NUMERIC, /* Option < AnyNumeric > */
	"arg_two" INT /* i32 */
) RETURNS internal /* Internal */
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'jb_num_jb_num_state_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:279
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" real[], /* Vec < Option < f32 > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_float4_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:284
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" real[], /* Vec < Option < f32 > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_float4_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:265
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" double precision[], /* Vec < Option < f64 > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_float8_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:270
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" double precision[], /* Vec < Option < f64 > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_float8_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:333
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" smallint[], /* Vec < Option < i16 > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int2_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:338
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" smallint[], /* Vec < Option < i16 > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int2_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:315
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" INT[], /* Vec < Option < i32 > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int4_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:320
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" INT[], /* Vec < Option < i32 > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int4_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:297
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" bigint[], /* Vec < Option < i64 > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int8_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:302
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" bigint[], /* Vec < Option < i64 > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_int8_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:351
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" NUMERIC[], /* Vec < Option < AnyNumeric > > */
	"breaks" INT /* i32 */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_numeric_array_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:359
-- pg_eviltransform::extension::st_jenksbins
CREATE  FUNCTION "st_jenksbins"(
	"values" NUMERIC[], /* Vec < Option < AnyNumeric > > */
	"breaks" INT, /* i32 */
	"invert" bool /* bool */
) RETURNS double precision[] /* :: std :: option :: Option < Vec < f64 > > */
IMMUTABLE STRICT PARALLEL SAFE 
LANGUAGE c /* Rust */
AS 'MODULE_PATHNAME', 'st_jenksbins_numeric_array_invert_wrapper';
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:416
-- pg_eviltransform::extension::JbF8
CREATE AGGREGATE st_jenksbins (
	double precision, /* Option < f64 > */
	INT /* i32 */
)
(
	SFUNC = "jb_f_8_jb_f_8_state", /* pg_eviltransform::extension::JbF8::state */
	STYPE = internal, /* Internal */
	FINALFUNC = "jb_f_8_jb_f_8_finalize" /* pg_eviltransform::extension::JbF8::final */
);
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:445
-- pg_eviltransform::extension::JbF8Inv
CREATE AGGREGATE st_jenksbins (
	double precision, /* Option < f64 > */
	INT, /* i32 */
	bool /* bool */
)
(
	SFUNC = "jb_f_8_inv_jb_f_8_inv_state", /* pg_eviltransform::extension::JbF8Inv::state */
	STYPE = internal, /* Internal */
	FINALFUNC = "jb_f_8_inv_jb_f_8_inv_finalize" /* pg_eviltransform::extension::JbF8Inv::final */
);
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:474
-- pg_eviltransform::extension::JbNum
CREATE AGGREGATE st_jenksbins (
	NUMERIC, /* Option < AnyNumeric > */
	INT /* i32 */
)
(
	SFUNC = "jb_num_jb_num_state", /* pg_eviltransform::extension::JbNum::state */
	STYPE = internal, /* Internal */
	FINALFUNC = "jb_num_jb_num_finalize" /* pg_eviltransform::extension::JbNum::final */
);
/* </end connected objects> */
/* <begin connected objects> */
-- src/lib.rs:503
-- pg_eviltransform::extension::JbNumInv
CREATE AGGREGATE st_jenksbins (
	NUMERIC, /* Option < AnyNumeric > */
	INT, /* i32 */
	bool /* bool */
)
(
	SFUNC = "jb_num_inv_jb_num_inv_state", /* pg_eviltransform::extension::JbNumInv::state */
	STYPE = internal, /* Internal */
	FINALFUNC = "jb_num_inv_jb_num_inv_finalize" /* pg_eviltransform::extension::JbNumInv::final */
);
/* </end connected objects> */
