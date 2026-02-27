use crate::coord::{TransformKind, apply};

const EWKB_Z: u32 = 0x8000_0000;
const EWKB_M: u32 = 0x4000_0000;
const EWKB_SRID: u32 = 0x2000_0000;
const EWKB_TYPE_MASK: u32 = 0x0000_FFFF;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EwkbError {
    UnexpectedEof,
    InvalidEndian(u8),
    UnsupportedType(u32),
    TrailingData(usize),
}

impl std::fmt::Display for EwkbError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EwkbError::UnexpectedEof => write!(f, "unexpected end of EWKB"),
            EwkbError::InvalidEndian(v) => write!(f, "invalid EWKB endian marker: {v}"),
            EwkbError::UnsupportedType(v) => write!(f, "unsupported EWKB geometry type: {v}"),
            EwkbError::TrailingData(n) => write!(f, "EWKB has {n} trailing bytes"),
        }
    }
}

impl std::error::Error for EwkbError {}

#[derive(Clone, Copy, Debug)]
enum Endian {
    Big,
    Little,
}

impl Endian {
    #[inline]
    fn from_marker(marker: u8) -> Result<Self, EwkbError> {
        match marker {
            0 => Ok(Self::Big),
            1 => Ok(Self::Little),
            other => Err(EwkbError::InvalidEndian(other)),
        }
    }

    #[inline]
    fn read_u32(self, bytes: [u8; 4]) -> u32 {
        match self {
            Self::Big => u32::from_be_bytes(bytes),
            Self::Little => u32::from_le_bytes(bytes),
        }
    }

    #[inline]
    fn read_f64(self, bytes: [u8; 8]) -> f64 {
        match self {
            Self::Big => f64::from_be_bytes(bytes),
            Self::Little => f64::from_le_bytes(bytes),
        }
    }

    #[inline]
    fn write_f64(self, value: f64) -> [u8; 8] {
        match self {
            Self::Big => value.to_be_bytes(),
            Self::Little => value.to_le_bytes(),
        }
    }
}

#[inline]
fn ensure_remaining(buf: &[u8], offset: usize, need: usize) -> Result<(), EwkbError> {
    if buf.len().saturating_sub(offset) < need {
        return Err(EwkbError::UnexpectedEof);
    }
    Ok(())
}

#[inline]
fn read_u8(buf: &[u8], offset: &mut usize) -> Result<u8, EwkbError> {
    ensure_remaining(buf, *offset, 1)?;
    let v = buf[*offset];
    *offset += 1;
    Ok(v)
}

#[inline]
fn read_u32(buf: &[u8], offset: &mut usize, endian: Endian) -> Result<u32, EwkbError> {
    ensure_remaining(buf, *offset, 4)?;
    let mut raw = [0u8; 4];
    raw.copy_from_slice(&buf[*offset..*offset + 4]);
    *offset += 4;
    Ok(endian.read_u32(raw))
}

#[inline]
fn read_f64(buf: &[u8], offset: &mut usize, endian: Endian) -> Result<f64, EwkbError> {
    ensure_remaining(buf, *offset, 8)?;
    let mut raw = [0u8; 8];
    raw.copy_from_slice(&buf[*offset..*offset + 8]);
    *offset += 8;
    Ok(endian.read_f64(raw))
}

#[inline]
fn write_f64(buf: &mut [u8], offset: usize, endian: Endian, value: f64) -> Result<(), EwkbError> {
    ensure_remaining(buf, offset, 8)?;
    buf[offset..offset + 8].copy_from_slice(&endian.write_f64(value));
    Ok(())
}

#[inline]
fn skip_bytes(buf: &[u8], offset: &mut usize, n: usize) -> Result<(), EwkbError> {
    ensure_remaining(buf, *offset, n)?;
    *offset += n;
    Ok(())
}

#[inline]
fn transform_coord_tuple(
    buf: &mut [u8],
    offset: &mut usize,
    endian: Endian,
    has_z: bool,
    has_m: bool,
    kind: TransformKind,
) -> Result<(), EwkbError> {
    let x_offset = *offset;
    let x = read_f64(buf, offset, endian)?;
    let y_offset = *offset;
    let y = read_f64(buf, offset, endian)?;

    let (lat, lng) = apply(kind, y, x);
    write_f64(buf, x_offset, endian, lng)?;
    write_f64(buf, y_offset, endian, lat)?;

    if has_z {
        skip_bytes(buf, offset, 8)?;
    }
    if has_m {
        skip_bytes(buf, offset, 8)?;
    }
    Ok(())
}

fn transform_point_array(
    buf: &mut [u8],
    offset: &mut usize,
    endian: Endian,
    has_z: bool,
    has_m: bool,
    kind: TransformKind,
) -> Result<(), EwkbError> {
    let npoints = read_u32(buf, offset, endian)? as usize;
    for _ in 0..npoints {
        transform_coord_tuple(buf, offset, endian, has_z, has_m, kind)?;
    }
    Ok(())
}

fn transform_polygon(
    buf: &mut [u8],
    offset: &mut usize,
    endian: Endian,
    has_z: bool,
    has_m: bool,
    kind: TransformKind,
) -> Result<(), EwkbError> {
    let nrings = read_u32(buf, offset, endian)? as usize;
    for _ in 0..nrings {
        transform_point_array(buf, offset, endian, has_z, has_m, kind)?;
    }
    Ok(())
}

fn transform_collection(
    buf: &mut [u8],
    offset: &mut usize,
    endian: Endian,
    kind: TransformKind,
) -> Result<(), EwkbError> {
    let ngeoms = read_u32(buf, offset, endian)? as usize;
    for _ in 0..ngeoms {
        transform_geometry(buf, offset, kind)?;
    }
    Ok(())
}

fn transform_geometry(
    buf: &mut [u8],
    offset: &mut usize,
    kind: TransformKind,
) -> Result<(), EwkbError> {
    let marker = read_u8(buf, offset)?;
    let endian = Endian::from_marker(marker)?;

    let type_word = read_u32(buf, offset, endian)?;
    let has_z = (type_word & EWKB_Z) != 0;
    let has_m = (type_word & EWKB_M) != 0;
    let has_srid = (type_word & EWKB_SRID) != 0;
    let gtype = type_word & EWKB_TYPE_MASK;

    if has_srid {
        let _ = read_u32(buf, offset, endian)?;
    }

    match gtype {
        1 => transform_coord_tuple(buf, offset, endian, has_z, has_m, kind),
        2 | 8 | 13 => transform_point_array(buf, offset, endian, has_z, has_m, kind),
        3 | 17 => transform_polygon(buf, offset, endian, has_z, has_m, kind),
        4 | 5 | 6 | 7 | 9 | 10 | 11 | 12 | 14 | 15 | 16 => {
            transform_collection(buf, offset, endian, kind)
        }
        _ => Err(EwkbError::UnsupportedType(gtype)),
    }
}

pub fn transform_ewkb_in_place(buf: &mut [u8], kind: TransformKind) -> Result<(), EwkbError> {
    let mut offset = 0usize;
    transform_geometry(buf, &mut offset, kind)?;
    if offset != buf.len() {
        return Err(EwkbError::TrailingData(buf.len() - offset));
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::coord::TransformKind;

    fn hex_to_bytes(hex: &str) -> Vec<u8> {
        let mut out = Vec::with_capacity(hex.len() / 2);
        let bytes = hex.as_bytes();
        let mut i = 0;
        while i + 1 < bytes.len() {
            let hi = (bytes[i] as char).to_digit(16).unwrap();
            let lo = (bytes[i + 1] as char).to_digit(16).unwrap();
            out.push(((hi << 4) | lo) as u8);
            i += 2;
        }
        out
    }

    fn read_le_f64(bytes: &[u8], offset: usize) -> f64 {
        let mut raw = [0u8; 8];
        raw.copy_from_slice(&bytes[offset..offset + 8]);
        f64::from_le_bytes(raw)
    }

    #[test]
    fn test_transform_point_wkb() {
        let mut ewkb = hex_to_bytes("01010000000000000000005E400000000000003E40");
        transform_ewkb_in_place(&mut ewkb, TransformKind::Wgs2Gcj).unwrap();

        let lng = read_le_f64(&ewkb, 5);
        let lat = read_le_f64(&ewkb, 13);
        assert!((lng - 120.004_660_445_597).abs() < 1e-6);
        assert!((lat - 29.997_534_331_696_1).abs() < 1e-6);
    }

    #[test]
    fn test_invalid_geometry_type() {
        let mut ewkb = vec![1, 0xFF, 0, 0, 0];
        let err = transform_ewkb_in_place(&mut ewkb, TransformKind::Wgs2Gcj).unwrap_err();
        assert_eq!(err, EwkbError::UnsupportedType(255));
    }
}
