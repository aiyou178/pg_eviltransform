use std::f64::consts::PI;

const EARTH_R: f64 = 6_378_137.0;
const EE: f64 = 0.006_693_421_622_965_943_23;
const X_PI: f64 = PI * 3000.0 / 180.0;

#[derive(Clone, Copy, Debug)]
pub enum TransformKind {
    Wgs2Gcj,
    Gcj2Wgs,
    Gcj2Bd,
    Bd2Gcj,
    Wgs2Bd,
    Bd2Wgs,
}

#[inline(always)]
fn out_of_china(lat: f64, lng: f64) -> bool {
    lng < 72.004 || lng > 137.8347 || lat < 0.8293 || lat > 55.8271
}

#[inline(always)]
fn transform(x: f64, y: f64) -> (f64, f64) {
    let xy = x * y;
    let abs_x = x.abs().sqrt();
    let x_pi = x * PI;
    let y_pi = y * PI;

    let mut d = 20.0 * (6.0 * x_pi).sin() + 20.0 * (2.0 * x_pi).sin();
    let mut lat = d;
    let mut lng = d;

    lat += 20.0 * y_pi.sin() + 40.0 * (y_pi / 3.0).sin();
    lng += 20.0 * x_pi.sin() + 40.0 * (x_pi / 3.0).sin();

    lat += 160.0 * (y_pi / 12.0).sin() + 320.0 * (y_pi / 30.0).sin();
    lng += 150.0 * (x_pi / 12.0).sin() + 300.0 * (x_pi / 30.0).sin();

    d = 2.0 / 3.0;
    lat *= d;
    lng *= d;

    lat += -100.0 + 2.0 * x + 3.0 * y + 0.2 * y * y + 0.1 * xy + 0.2 * abs_x;
    lng += 300.0 + x + 2.0 * y + 0.1 * x * x + 0.1 * xy + 0.1 * abs_x;

    (lat, lng)
}

#[inline(always)]
fn delta(lat: f64, lng: f64) -> (f64, f64) {
    let (mut d_lat, mut d_lng) = transform(lng - 105.0, lat - 35.0);
    let rad_lat = lat.to_radians();
    let sin_lat = rad_lat.sin();
    let magic = 1.0 - EE * sin_lat * sin_lat;
    let sqrt_magic = magic.sqrt();

    d_lat = (d_lat * 180.0) / (((EARTH_R * (1.0 - EE)) / (magic * sqrt_magic)) * PI);
    d_lng = (d_lng * 180.0) / ((EARTH_R / sqrt_magic) * rad_lat.cos() * PI);

    (d_lat, d_lng)
}

#[inline(always)]
pub fn wgs2gcj(lat: f64, lng: f64) -> (f64, f64) {
    if out_of_china(lat, lng) {
        return (lat, lng);
    }
    let (d_lat, d_lng) = delta(lat, lng);
    (lat + d_lat, lng + d_lng)
}

#[inline(always)]
pub fn gcj2wgs(lat: f64, lng: f64) -> (f64, f64) {
    if out_of_china(lat, lng) {
        return (lat, lng);
    }
    let (d_lat, d_lng) = delta(lat, lng);
    (lat - d_lat, lng - d_lng)
}

#[inline(always)]
pub fn gcj2bd(lat: f64, lng: f64) -> (f64, f64) {
    if out_of_china(lat, lng) {
        return (lat, lng);
    }

    let z = lng.hypot(lat) + 0.00002 * (lat * X_PI).sin();
    let theta = lat.atan2(lng) + 0.000003 * (lng * X_PI).cos();
    (z * theta.sin() + 0.006, z * theta.cos() + 0.0065)
}

#[inline(always)]
pub fn bd2gcj(lat: f64, lng: f64) -> (f64, f64) {
    if out_of_china(lat, lng) {
        return (lat, lng);
    }

    let x = lng - 0.0065;
    let y = lat - 0.006;
    let z = x.hypot(y) - 0.00002 * (y * X_PI).sin();
    let theta = y.atan2(x) - 0.000003 * (x * X_PI).cos();
    (z * theta.sin(), z * theta.cos())
}

#[inline(always)]
pub fn wgs2bd(lat: f64, lng: f64) -> (f64, f64) {
    let (gcj_lat, gcj_lng) = wgs2gcj(lat, lng);
    gcj2bd(gcj_lat, gcj_lng)
}

#[inline(always)]
pub fn bd2wgs(lat: f64, lng: f64) -> (f64, f64) {
    let (gcj_lat, gcj_lng) = bd2gcj(lat, lng);
    gcj2wgs(gcj_lat, gcj_lng)
}

#[inline(always)]
pub fn apply(kind: TransformKind, lat: f64, lng: f64) -> (f64, f64) {
    match kind {
        TransformKind::Wgs2Gcj => wgs2gcj(lat, lng),
        TransformKind::Gcj2Wgs => gcj2wgs(lat, lng),
        TransformKind::Gcj2Bd => gcj2bd(lat, lng),
        TransformKind::Bd2Gcj => bd2gcj(lat, lng),
        TransformKind::Wgs2Bd => wgs2bd(lat, lng),
        TransformKind::Bd2Wgs => bd2wgs(lat, lng),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_nearly(a: f64, b: f64) {
        assert!((a - b).abs() < 1e-6, "{a} != {b}");
    }

    #[test]
    fn test_wgs2gcj_reference_point() {
        let (lat, lng) = wgs2gcj(39.915, 116.404);
        assert_nearly(lat, 39.916_404_281_501_64);
        assert_nearly(lng, 116.410_244_499_169_38);
    }

    #[test]
    fn test_gcj2bd_reference_point() {
        let (lat, lng) = gcj2bd(39.915, 116.404);
        assert_nearly(lat, 39.921_336_993_510_21);
        assert_nearly(lng, 116.410_369_493_710_29);
    }

    #[test]
    fn test_out_of_china_not_changed() {
        let (lat, lng) = wgs2gcj(30.0, -120.0);
        assert_eq!(lat, 30.0);
        assert_eq!(lng, -120.0);
    }
}
