#include <math.h>
#include <stdarg.h>
#include <stddef.h>
#include <stdint.h>
#include <stdlib.h>
#include <string.h>

#include <librttopo_geom.h>

#define EARTH_R 6378137.0
#define EE 0.00669342162296594323
#define X_PI (M_PI * 3000.0 / 180.0)

#define MODE_WGS2GCJ 1
#define MODE_GCJ2WGS 2
#define MODE_WGS2BD 3
#define MODE_BD2WGS 4
#define MODE_GCJ2BD 5
#define MODE_BD2GCJ 6

#define PGCT_G2FLAG_Z 0x01u
#define PGCT_G2FLAG_M 0x02u
#define PGCT_G2FLAG_VER_0 0x40u
#define PGCT_G2FLAG_EXTENDED 0x80u

static void pgct_noop_reporter(const char* fmt, va_list ap, void* arg) {
  (void)fmt;
  (void)ap;
  (void)arg;
}

static RTCTX* pgct_ctx(void) {
  static RTCTX* ctx = NULL;
  if (ctx == NULL) {
    ctx = rtgeom_init(malloc, realloc, free);
    if (ctx != NULL) {
      rtgeom_set_error_logger(ctx, pgct_noop_reporter, NULL);
      rtgeom_set_notice_logger(ctx, pgct_noop_reporter, NULL);
    }
  }
  return ctx;
}

static int out_of_china(double lat, double lng) {
  return !(lng >= 72.004 && lng <= 137.8347 && lat >= 0.8293 && lat <= 55.8271);
}

static void transform(double x, double y, double* lat, double* lng) {
  double xy = x * y;
  double abs_x = sqrt(fabs(x));
  double x_pi = x * M_PI;
  double y_pi = y * M_PI;
  double d = 20.0 * sin(6.0 * x_pi) + 20.0 * sin(2.0 * x_pi);

  *lat = d;
  *lng = d;

  *lat += 20.0 * sin(y_pi) + 40.0 * sin(y_pi / 3.0);
  *lng += 20.0 * sin(x_pi) + 40.0 * sin(x_pi / 3.0);

  *lat += 160.0 * sin(y_pi / 12.0) + 320.0 * sin(y_pi / 30.0);
  *lng += 150.0 * sin(x_pi / 12.0) + 300.0 * sin(x_pi / 30.0);

  *lat *= 2.0 / 3.0;
  *lng *= 2.0 / 3.0;

  *lat += -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * xy + 0.2 * abs_x;
  *lng += 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * xy + 0.1 * abs_x;
}

static void delta(double lat, double lng, double* d_lat, double* d_lng) {
  double rad_lat;
  double magic;
  double sqrt_magic;

  transform(lng - 105.0, lat - 35.0, d_lat, d_lng);
  rad_lat = lat / 180.0 * M_PI;
  magic = sin(rad_lat);
  magic = 1.0 - EE * magic * magic;
  sqrt_magic = sqrt(magic);
  *d_lat = (*d_lat * 180.0) / ((EARTH_R * (1.0 - EE)) / (magic * sqrt_magic) * M_PI);
  *d_lng = (*d_lng * 180.0) / (EARTH_R / sqrt_magic * cos(rad_lat) * M_PI);
}

static void wgs2gcj(double lat, double lng, double* out_lat, double* out_lng) {
  double d_lat;
  double d_lng;
  if (out_of_china(lat, lng)) {
    *out_lat = lat;
    *out_lng = lng;
    return;
  }
  delta(lat, lng, &d_lat, &d_lng);
  *out_lat = lat + d_lat;
  *out_lng = lng + d_lng;
}

static void gcj2wgs(double lat, double lng, double* out_lat, double* out_lng) {
  double d_lat;
  double d_lng;
  if (out_of_china(lat, lng)) {
    *out_lat = lat;
    *out_lng = lng;
    return;
  }
  delta(lat, lng, &d_lat, &d_lng);
  *out_lat = lat - d_lat;
  *out_lng = lng - d_lng;
}

static void gcj2bd(double lat, double lng, double* out_lat, double* out_lng) {
  double z;
  double theta;
  if (out_of_china(lat, lng)) {
    *out_lat = lat;
    *out_lng = lng;
    return;
  }
  z = sqrt(lng * lng + lat * lat) + 0.00002 * sin(lat * X_PI);
  theta = atan2(lat, lng) + 0.000003 * cos(lng * X_PI);
  *out_lng = z * cos(theta) + 0.0065;
  *out_lat = z * sin(theta) + 0.006;
}

static void bd2gcj(double lat, double lng, double* out_lat, double* out_lng) {
  double x;
  double y;
  double z;
  double theta;
  if (out_of_china(lat, lng)) {
    *out_lat = lat;
    *out_lng = lng;
    return;
  }
  x = lng - 0.0065;
  y = lat - 0.006;
  z = sqrt(x * x + y * y) - 0.00002 * sin(y * X_PI);
  theta = atan2(y, x) - 0.000003 * cos(x * X_PI);
  *out_lng = z * cos(theta);
  *out_lat = z * sin(theta);
}

static void apply_mode(int mode, double lat, double lng, double* out_lat, double* out_lng) {
  double gcj_lat;
  double gcj_lng;
  switch (mode) {
    case MODE_WGS2GCJ:
      wgs2gcj(lat, lng, out_lat, out_lng);
      return;
    case MODE_GCJ2WGS:
      gcj2wgs(lat, lng, out_lat, out_lng);
      return;
    case MODE_GCJ2BD:
      gcj2bd(lat, lng, out_lat, out_lng);
      return;
    case MODE_BD2GCJ:
      bd2gcj(lat, lng, out_lat, out_lng);
      return;
    case MODE_WGS2BD:
      wgs2gcj(lat, lng, &gcj_lat, &gcj_lng);
      gcj2bd(gcj_lat, gcj_lng, out_lat, out_lng);
      return;
    case MODE_BD2WGS:
      bd2gcj(lat, lng, &gcj_lat, &gcj_lng);
      gcj2wgs(gcj_lat, gcj_lng, out_lat, out_lng);
      return;
    default:
      *out_lat = lat;
      *out_lng = lng;
      return;
  }
}

int pgct_gserialized_get_srid(const uint8_t* input, int32_t* out_srid) {
  RTCTX* ctx = pgct_ctx();
  if (ctx == NULL || input == NULL || out_srid == NULL) {
    return 1;
  }

  *out_srid = gserialized_get_srid(ctx, (const GSERIALIZED*)input);
  return 0;
}

/* Check whether a detoasted 4-byte-header GSERIALIZED is a Cartesian POINT
 * without bbox, geodetic, solid, or v2 extended flags. In both PostGIS v1 and
 * v2 GSERIALIZED layouts, bytes 8..16 are gtype/npoints and coordinates begin
 * at byte 16. */
int pgct_can_transform_point_in_place(const uint8_t* input, size_t len) {
  uint8_t flags;
  uint32_t gtype;
  uint32_t npoints;
  size_t ndims;

  if (input == NULL || len < 16) {
    return 0;
  }

  flags = input[7];
  /* Allow only Z, M, and the GSERIALIZED v2 version bit; reject bbox, geodetic,
   * solid, extended-flags, or any unknown flag bits. */
  if ((flags & ~(PGCT_G2FLAG_Z | PGCT_G2FLAG_M | PGCT_G2FLAG_VER_0)) != 0) {
    return 0;
  }

  memcpy(&gtype, input + 8, sizeof(uint32_t));
  if (gtype != RTPOINTTYPE) {
    return 0;
  }

  memcpy(&npoints, input + 12, sizeof(uint32_t));
  if (npoints == 0) {
    return len == 16;
  }
  if (npoints != 1) {
    return 0;
  }

  ndims = 2u + (size_t)(flags & 0x01u) + (size_t)((flags >> 1) & 0x01u);
  return len == 16u + ndims * sizeof(double);
}

int pgct_transform_point_in_place(uint8_t* buf,
                                  size_t len,
                                  int mode,
                                  int32_t dst_srid) {
  RTCTX* ctx = pgct_ctx();
  uint32_t npoints;

  if (ctx == NULL || !pgct_can_transform_point_in_place(buf, len)) {
    return 1;
  }

  memcpy(&npoints, buf + 12, sizeof(uint32_t));
  if (npoints == 1) {
    double x;
    double y;
    double out_lat;
    double out_lng;
    memcpy(&x, buf + 16, sizeof(double));
    memcpy(&y, buf + 24, sizeof(double));
    apply_mode(mode, y, x, &out_lat, &out_lng);
    memcpy(buf + 16, &out_lng, sizeof(double));
    memcpy(buf + 24, &out_lat, sizeof(double));
  }

  gserialized_set_srid(ctx, (GSERIALIZED*)buf, dst_srid);
  return 0;
}

static int transform_pointarray(const RTCTX* ctx, RTPOINTARRAY* points, int mode) {
  size_t stride;
  uint8_t* cursor;
  (void)ctx;

  if (points == NULL || points->npoints <= 0) {
    return 0;
  }
  if (points->serialized_pointlist == NULL) {
    return 4;
  }

  stride = (size_t)RTFLAGS_NDIMS(points->flags) * sizeof(double);
  cursor = points->serialized_pointlist;
  for (int i = 0; i < points->npoints; i++, cursor += stride) {
    double x;
    double y;
    double out_lat;
    double out_lng;
    memcpy(&x, cursor, sizeof(double));
    memcpy(&y, cursor + sizeof(double), sizeof(double));
    apply_mode(mode, y, x, &out_lat, &out_lng);
    memcpy(cursor, &out_lng, sizeof(double));
    memcpy(cursor + sizeof(double), &out_lat, sizeof(double));
  }
  return 0;
}

static int can_traverse_cartesian(const RTGEOM* geom, int depth) {
  if (geom == NULL) {
    return 1;
  }
  if (depth > 64) {
    return 0;
  }

  switch (geom->type) {
    case RTPOINTTYPE:
    case RTLINETYPE:
    case RTTRIANGLETYPE:
    case RTCIRCSTRINGTYPE:
    case RTPOLYGONTYPE:
      return 1;
    case RTCURVEPOLYTYPE: {
      const RTCURVEPOLY* cpoly = (const RTCURVEPOLY*)geom;
      for (int i = 0; i < cpoly->nrings; i++) {
        if (!can_traverse_cartesian(cpoly->rings[i], depth + 1)) {
          return 0;
        }
      }
      return 1;
    }
    case RTMULTIPOINTTYPE:
    case RTMULTILINETYPE:
    case RTMULTIPOLYGONTYPE:
    case RTCOLLECTIONTYPE:
    case RTCOMPOUNDTYPE:
    case RTMULTICURVETYPE:
    case RTMULTISURFACETYPE:
    case RTPOLYHEDRALSURFACETYPE:
    case RTTINTYPE: {
      const RTCOLLECTION* col = (const RTCOLLECTION*)geom;
      for (int i = 0; i < col->ngeoms; i++) {
        if (!can_traverse_cartesian(col->geoms[i], depth + 1)) {
          return 0;
        }
      }
      return 1;
    }
    default:
      return 0;
  }
}

static int transform_cartesian_geom(const RTCTX* ctx, RTGEOM* geom, int mode) {
  if (geom == NULL) {
    return 0;
  }

  switch (geom->type) {
    case RTPOINTTYPE:
      return transform_pointarray(ctx, ((RTPOINT*)geom)->point, mode);
    case RTLINETYPE:
      return transform_pointarray(ctx, ((RTLINE*)geom)->points, mode);
    case RTTRIANGLETYPE:
      return transform_pointarray(ctx, ((RTTRIANGLE*)geom)->points, mode);
    case RTCIRCSTRINGTYPE:
      return transform_pointarray(ctx, ((RTCIRCSTRING*)geom)->points, mode);
    case RTPOLYGONTYPE: {
      RTPOLY* poly = (RTPOLY*)geom;
      for (int i = 0; i < poly->nrings; i++) {
        int rc = transform_pointarray(ctx, poly->rings[i], mode);
        if (rc != 0) {
          return rc;
        }
      }
      return 0;
    }
    case RTCURVEPOLYTYPE: {
      RTCURVEPOLY* cpoly = (RTCURVEPOLY*)geom;
      for (int i = 0; i < cpoly->nrings; i++) {
        int rc = transform_cartesian_geom(ctx, cpoly->rings[i], mode);
        if (rc != 0) {
          return rc;
        }
      }
      return 0;
    }
    case RTMULTIPOINTTYPE:
    case RTMULTILINETYPE:
    case RTMULTIPOLYGONTYPE:
    case RTCOLLECTIONTYPE:
    case RTCOMPOUNDTYPE:
    case RTMULTICURVETYPE:
    case RTMULTISURFACETYPE:
    case RTPOLYHEDRALSURFACETYPE:
    case RTTINTYPE: {
      RTCOLLECTION* col = (RTCOLLECTION*)geom;
      for (int i = 0; i < col->ngeoms; i++) {
        int rc = transform_cartesian_geom(ctx, col->geoms[i], mode);
        if (rc != 0) {
          return rc;
        }
      }
      return 0;
    }
    default:
      return 4;
  }
}

static int transform_coordinates(const RTCTX* ctx, RTGEOM* geom, int mode) {
  RTPOINTITERATOR* iter = NULL;

  /* Traverse Cartesian geometries directly without iterator stack allocations
   * or per-vertex shared-library calls; keep the generic iterator for geodetic
   * storage, deeply nested collections, or unrecognized types. */
  if (!RTFLAGS_GET_GEODETIC(geom->flags) && can_traverse_cartesian(geom, 0)) {
    return transform_cartesian_geom(ctx, geom, mode);
  }

  iter = rtpointiterator_create_rw(ctx, geom);
  if (iter == NULL) {
    return 3;
  }

  while (rtpointiterator_has_next(ctx, iter) == RT_TRUE) {
    RTPOINT4D point;
    double out_lat;
    double out_lng;

    if (rtpointiterator_peek(ctx, iter, &point) != RT_SUCCESS) {
      rtpointiterator_destroy(ctx, iter);
      return 4;
    }

    apply_mode(mode, point.y, point.x, &out_lat, &out_lng);
    point.x = out_lng;
    point.y = out_lat;

    if (rtpointiterator_modify_next(ctx, iter, &point) != RT_SUCCESS) {
      rtpointiterator_destroy(ctx, iter);
      return 5;
    }
  }

  rtpointiterator_destroy(ctx, iter);
  return 0;
}

int pgct_transform_gserialized(const uint8_t* input,
                               int mode,
                               int32_t dst_srid,
                               uint8_t** out_ptr,
                               size_t* out_len) {
  RTCTX* ctx = pgct_ctx();
  RTGEOM* geom = NULL;
  GSERIALIZED* out = NULL;
  size_t out_size = 0;

  if (ctx == NULL || input == NULL || out_ptr == NULL || out_len == NULL) {
    return 1;
  }

  /* librttopo only implements GSERIALIZED v1 and cannot parse PostGIS v2
   * extended-flag headers safely. */
  if ((input[7] & PGCT_G2FLAG_EXTENDED) != 0) {
    return 7;
  }

  geom = rtgeom_from_gserialized(ctx, (const GSERIALIZED*)input);
  if (geom == NULL) {
    return 2;
  }

  int rc = transform_coordinates(ctx, geom, mode);
  if (rc != 0) {
    rtgeom_free(ctx, geom);
    return rc;
  }

  /* The deserialized box still describes the input coordinates. */
  if (geom->bbox != NULL) {
    rtgeom_drop_bbox(ctx, geom);
    rtgeom_add_bbox(ctx, geom);
  }

  out = gserialized_from_rtgeom(
      ctx,
      geom,
      gserialized_is_geodetic(ctx, (const GSERIALIZED*)input),
      &out_size);
  rtgeom_free(ctx, geom);
  geom = NULL;

  if (out == NULL || out_size == 0) {
    return 6;
  }

  gserialized_set_srid(ctx, out, dst_srid);

  /* pgct_ctx installs malloc/free, so transfer this buffer to the Rust caller. */
  *out_ptr = (uint8_t*)out;
  *out_len = out_size;
  return 0;
}

void pgct_free(void* ptr) {
  free(ptr);
}
