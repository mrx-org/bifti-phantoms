//! Deterministic NIfTI-1 writer.
//!
//! The `nifti` crate (built here without `ndarray`) cannot write, and its header defaults
//! would have to be audited for the orientation fields NIFTI.md requires. A phantom
//! directory is hashed as a whole, so the bytes must also be reproducible: fixed header
//! fields, little-endian, no extensions, and a gzip header without mtime or file name.

use std::{
    fs::File,
    io::{BufWriter, Write},
    mem::discriminant,
    path::Path,
};

use flate2::{Compression, GzBuilder};
use num_complex::Complex;

use crate::{Error, Volume, VolumeData, volume::for_each_variant};

const HEADER_SIZE: usize = 348;
/// 348-byte header + 4-byte (empty) extension flag.
const VOX_OFFSET: usize = 352;

const DT_COMPLEX128: i16 = 1792;
const DT_FLOAT64: i16 = 64;

/// gzip header "operating system" byte: 255 = unknown, so it does not depend on the host.
const GZIP_OS_UNKNOWN: u8 = 255;
const GZIP_LEVEL: u32 = 6;

trait LeBytes: Copy {
    fn push_le(self, out: &mut Vec<u8>);
}

macro_rules! le_bytes_for_numbers {
    ($($ty:ty),*) => {$(
        impl LeBytes for $ty {
            fn push_le(self, out: &mut Vec<u8>) {
                out.extend_from_slice(&self.to_le_bytes());
            }
        }
    )*};
}

le_bytes_for_numbers!(u8, u16, u32, u64, i8, i16, i32, i64, f32, f64);

impl<T: LeBytes> LeBytes for Complex<T> {
    fn push_le(self, out: &mut Vec<u8>) {
        self.re.push_le(out);
        self.im.push_le(out);
    }
}

/// NIfTI `(datatype, bitpix)` of a volume's element type.
fn datatype(data: &VolumeData) -> (i16, i16) {
    match data {
        VolumeData::Uint8(_) => (2, 8),
        VolumeData::Int16(_) => (4, 16),
        VolumeData::Int32(_) => (8, 32),
        VolumeData::Float32(_) => (16, 32),
        VolumeData::Complex64(_) => (32, 64),
        VolumeData::Float64(_) => (DT_FLOAT64, 64),
        VolumeData::Int8(_) => (256, 8),
        VolumeData::Uint16(_) => (512, 16),
        VolumeData::Uint32(_) => (768, 32),
        VolumeData::Int64(_) => (1024, 64),
        VolumeData::Uint64(_) => (1280, 64),
        VolumeData::Complex128(_) => (DT_COMPLEX128, 128),
    }
}

/// The element type all volumes of a stack are stored as.
#[derive(Clone, Copy)]
enum StackType {
    /// Every volume has the same element type: keep it.
    Native,
    /// Mixed real types.
    Float64,
    /// Mixed types, at least one complex.
    Complex128,
}

impl StackType {
    fn of(volumes: &[&Volume]) -> Self {
        let first = discriminant(&volumes[0].data);
        if volumes.iter().all(|v| discriminant(&v.data) == first) {
            Self::Native
        } else if volumes
            .iter()
            .any(|v| matches!(v.data, VolumeData::Complex64(_) | VolumeData::Complex128(_)))
        {
            Self::Complex128
        } else {
            Self::Float64
        }
    }

    fn datatype(self, first: &VolumeData) -> (i16, i16) {
        match self {
            Self::Native => datatype(first),
            Self::Float64 => (DT_FLOAT64, 64),
            Self::Complex128 => (DT_COMPLEX128, 128),
        }
    }
}

/// Append `data` to `out` as little-endian bytes, converted to `target`.
fn encode(data: &VolumeData, target: StackType, out: &mut Vec<u8>) {
    match target {
        StackType::Native => for_each_variant!(data, |items| {
            for &x in items {
                x.push_le(out);
            }
        }),
        StackType::Float64 => {
            // Only reached for real element types, where `to_f64` is the exact value.
            for x in data.to_f64_vec() {
                x.push_le(out);
            }
        }
        StackType::Complex128 => match data {
            VolumeData::Complex64(items) => {
                for x in items {
                    Complex::new(x.re as f64, x.im as f64).push_le(out);
                }
            }
            VolumeData::Complex128(items) => {
                for &x in items {
                    x.push_le(out);
                }
            }
            real => {
                for x in real.to_f64_vec() {
                    Complex::new(x, 0.0).push_le(out);
                }
            }
        },
    }
}

fn put_i16(header: &mut [u8], at: usize, v: i16) {
    header[at..at + 2].copy_from_slice(&v.to_le_bytes());
}

fn put_f32(header: &mut [u8], at: usize, v: f32) {
    header[at..at + 4].copy_from_slice(&v.to_le_bytes());
}

/// The 352 bytes preceding the voxel data: header plus the empty extension flag.
///
/// Orientation lives in the sform (`sform_code = 2`, aligned) only; the qform is unused
/// (`qform_code = 0`), as NIFTI.md requires.
fn header(
    shape: [usize; 3],
    n_volumes: usize,
    affine: &[[f64; 4]; 3],
    (datatype, bitpix): (i16, i16),
) -> Vec<u8> {
    let mut h = vec![0u8; VOX_OFFSET];
    h[0..4].copy_from_slice(&(HEADER_SIZE as i32).to_le_bytes());

    for (i, extent) in [4, shape[0], shape[1], shape[2], n_volumes, 1, 1, 1]
        .into_iter()
        .enumerate()
    {
        put_i16(&mut h, 40 + 2 * i, extent as i16);
    }
    put_i16(&mut h, 70, datatype);
    put_i16(&mut h, 72, bitpix);

    // pixdim: [qfac, dx, dy, dz, dt, ..]; voxel sizes are the affine's column norms.
    put_f32(&mut h, 76, 1.0);
    for axis in 0..3 {
        let norm = affine
            .iter()
            .map(|row| row[axis].powi(2))
            .sum::<f64>()
            .sqrt();
        put_f32(&mut h, 80 + 4 * axis, norm as f32);
    }
    put_f32(&mut h, 92, 1.0);

    put_f32(&mut h, 108, VOX_OFFSET as f32); // vox_offset
    put_f32(&mut h, 112, 1.0); // scl_slope
    h[123] = 2; // xyzt_units: millimetres

    put_i16(&mut h, 252, 0); // qform_code
    put_i16(&mut h, 254, 2); // sform_code
    for (row, base) in affine.iter().zip([280, 296, 312]) {
        for (col, &value) in row.iter().enumerate() {
            put_f32(&mut h, base + 4 * col, value as f32);
        }
    }
    h[344..348].copy_from_slice(b"n+1\0");
    h
}

/// Write `volumes` as one 4-D NIfTI-1 file, volumes stacked along the 4th dimension, and
/// return the index each input volume has in the file.
///
/// All volumes must share shape and affine. Volumes whose stored bytes are identical share
/// one index and are written once. Mixed element types are promoted to `Float64` (or
/// `Complex128` if any is complex). A path ending in `.gz` is gzip-compressed.
pub(crate) fn write_stack(path: &Path, volumes: &[&Volume]) -> Result<Vec<usize>, Error> {
    let Some(first) = volumes.first() else {
        return Err(Error::SaveError(format!(
            "{}: no volumes to write",
            path.display()
        )));
    };
    let n_voxels = first.shape.iter().product::<usize>();
    let stack_type = StackType::of(volumes);

    let mut payload: Vec<u8> = Vec::new();
    let mut stored_len = 0;
    let mut n_stored = 0;
    let mut slots = Vec::with_capacity(volumes.len());
    let mut bytes = Vec::new();

    for volume in volumes {
        if volume.shape != first.shape || volume.affine != first.affine {
            return Err(Error::SaveError(format!(
                "{}: volumes differ in shape or affine ({:?} vs {:?}), cannot stack them",
                path.display(),
                volume.shape,
                first.shape
            )));
        }
        bytes.clear();
        encode(&volume.data, stack_type, &mut bytes);
        if volume.data.len() == 1 && n_voxels > 1 {
            // A scalar-valued `func` (e.g. `x_mean`) leaves a single value for the volume.
            bytes = bytes.repeat(n_voxels);
        } else if volume.data.len() != n_voxels {
            return Err(Error::SaveError(format!(
                "{}: volume has {} values but shape {:?}",
                path.display(),
                volume.data.len(),
                volume.shape
            )));
        }

        if stored_len == 0 {
            stored_len = bytes.len();
        }
        let existing = (stored_len > 0)
            .then(|| {
                payload
                    .chunks_exact(stored_len)
                    .position(|stored| stored == bytes.as_slice())
            })
            .flatten();
        slots.push(existing.unwrap_or_else(|| {
            payload.extend_from_slice(&bytes);
            n_stored += 1;
            n_stored - 1
        }));
    }

    let header = header(
        first.shape,
        n_stored,
        &first.affine,
        stack_type.datatype(&first.data),
    );

    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let file = BufWriter::new(File::create(path)?);
    if path.extension().is_some_and(|ext| ext == "gz") {
        let mut gz = GzBuilder::new()
            .mtime(0)
            .operating_system(GZIP_OS_UNKNOWN)
            .write(file, Compression::new(GZIP_LEVEL));
        gz.write_all(&header)?;
        gz.write_all(&payload)?;
        gz.finish()?.flush()?;
    } else {
        let mut file = file;
        file.write_all(&header)?;
        file.write_all(&payload)?;
        file.flush()?;
    }
    Ok(slots)
}

impl Volume {
    /// Write this volume as a NIfTI-1 file with a singleton 4th dimension.
    ///
    /// The orientation is stored as an sform (`sform_code = 2`) from [`Volume::affine`]; the
    /// element type follows the [`VolumeData`] variant. A path ending in `.gz` is
    /// compressed. The bytes written depend only on the volume, so identical volumes give
    /// identical files.
    pub fn save_nifti<P: AsRef<Path>>(&self, path: P) -> Result<(), Error> {
        write_stack(path.as_ref(), &[self]).map(|_| ())
    }

    /// Write several volumes of the same grid as one NIfTI-1 file stacked along the 4th
    /// dimension, as BIfTI phantoms store one property for all tissues.
    ///
    /// Returns the 4th-dimension index of each input; bit-identical volumes share an index.
    /// See [`Volume::save_nifti`].
    pub fn save_nifti_stack<P: AsRef<Path>>(
        volumes: &[&Volume],
        path: P,
    ) -> Result<Vec<usize>, Error> {
        write_stack(path.as_ref(), volumes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn volume(data: VolumeData) -> Volume {
        Volume {
            affine: [
                [2.0, 0.0, 0.0, -1.0],
                [0.0, 3.0, 0.0, 0.5],
                [0.0, 0.0, 4.0, 0.0],
            ],
            shape: [2, 1, 1],
            data,
        }
    }

    #[test]
    fn header_fields() {
        let h = header(
            [2, 1, 1],
            3,
            &volume(VolumeData::Uint8(vec![0, 0])).affine,
            (2, 8),
        );
        assert_eq!(h.len(), 352);
        assert_eq!(&h[344..348], b"n+1\0");
        assert_eq!(i16::from_le_bytes([h[40], h[41]]), 4);
        assert_eq!(i16::from_le_bytes([h[48], h[49]]), 3); // dim[4]
        assert_eq!(i16::from_le_bytes([h[252], h[253]]), 0); // qform_code
        assert_eq!(i16::from_le_bytes([h[254], h[255]]), 2); // sform_code
        assert_eq!(f32::from_le_bytes(h[84..88].try_into().unwrap()), 3.0); // pixdim[2]
    }

    #[test]
    fn identical_volumes_share_a_slot_and_types_promote() {
        let dir = std::env::temp_dir().join(format!("bifti_nifti_write_{}", std::process::id()));
        let a = volume(VolumeData::Float32(vec![1.0, 2.0]));
        let b = volume(VolumeData::Float32(vec![3.0, 4.0]));
        let slots = write_stack(&dir.join("s.nii"), &[&a, &b, &a]).unwrap();
        assert_eq!(slots, [0, 1, 0]);
        let len = std::fs::metadata(dir.join("s.nii")).unwrap().len();
        assert_eq!(len, 352 + 2 * 2 * 4);

        let c = volume(VolumeData::Uint8(vec![1, 5]));
        let slots = write_stack(&dir.join("m.nii"), &[&a, &c]).unwrap();
        assert_eq!(slots, [0, 1]);
        let bytes = std::fs::read(dir.join("m.nii")).unwrap();
        assert_eq!(i16::from_le_bytes([bytes[70], bytes[71]]), DT_FLOAT64);
        assert_eq!(bytes.len(), 352 + 2 * 2 * 8);
    }
}
