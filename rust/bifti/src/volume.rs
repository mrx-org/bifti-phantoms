use num_complex::Complex;

pub struct Volume {
    pub affine: [[f64; 4]; 3],
    pub shape: [usize; 3],
    pub data: VolumeData,
}

impl Volume {
    pub fn single_voxel<T: VolumeDataElement>(value: T) -> Self {
        Self {
            affine: [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            shape: [1, 1, 1],
            data: value.into_volume(),
        }
    }
}

pub enum VolumeData {
    Uint8(Vec<u8>),
    Uint16(Vec<u16>),
    Uint32(Vec<u32>),
    Uint64(Vec<u64>),
    Int8(Vec<i8>),
    Int16(Vec<i16>),
    Int32(Vec<i32>),
    Int64(Vec<i64>),
    Float32(Vec<f32>),
    Float64(Vec<f64>),
    Complex64(Vec<Complex<f32>>),
    Complex128(Vec<Complex<f64>>),
    // Not supported: Float128, Complex256, Rgb24, Rgba32
}

impl VolumeData {
    /// Expands a single voxel volume into the given size. Panics on size != 1
    pub fn expand(self, n: usize) -> Self {
        assert_eq!(self.len(), 1);
        match self {
            VolumeData::Uint8(items) => items[0].fill_volume(n),
            VolumeData::Uint16(items) => items[0].fill_volume(n),
            VolumeData::Uint32(items) => items[0].fill_volume(n),
            VolumeData::Uint64(items) => items[0].fill_volume(n),
            VolumeData::Int8(items) => items[0].fill_volume(n),
            VolumeData::Int16(items) => items[0].fill_volume(n),
            VolumeData::Int32(items) => items[0].fill_volume(n),
            VolumeData::Int64(items) => items[0].fill_volume(n),
            VolumeData::Float32(items) => items[0].fill_volume(n),
            VolumeData::Float64(items) => items[0].fill_volume(n),
            VolumeData::Complex64(items) => items[0].fill_volume(n),
            VolumeData::Complex128(items) => items[0].fill_volume(n),
        }
    }

    pub fn len(&self) -> usize {
        match self {
            VolumeData::Uint8(items) => items.len(),
            VolumeData::Uint16(items) => items.len(),
            VolumeData::Uint32(items) => items.len(),
            VolumeData::Uint64(items) => items.len(),
            VolumeData::Int8(items) => items.len(),
            VolumeData::Int16(items) => items.len(),
            VolumeData::Int32(items) => items.len(),
            VolumeData::Int64(items) => items.len(),
            VolumeData::Float32(items) => items.len(),
            VolumeData::Float64(items) => items.len(),
            VolumeData::Complex64(items) => items.len(),
            VolumeData::Complex128(items) => items.len(),
        }
    }
}

pub trait VolumeDataElement: Sized + Copy {
    const ZERO: Self;

    fn into_volume(self) -> VolumeData
    where
        Self: Sized,
    {
        self.fill_volume(1)
    }

    /// Linearly interpolates between `a` (`t = 0`) and `b` (`t = 1`).
    fn lerp(a: Self, b: Self, t: f64) -> Self;

    fn fill_volume(self, n: usize) -> VolumeData;
}

macro_rules! impl_volume_data_element {
    ($($ty:ty => ($variant:ident, $zero:expr, |$a:ident, $b:ident, $t:ident| $lerp:expr)),* $(,)?) => {
        $(
            impl VolumeDataElement for $ty {
                const ZERO: Self = $zero;

                fn lerp($a: Self, $b: Self, $t: f64) -> Self {
                    $lerp
                }

                fn fill_volume(self, n: usize) -> VolumeData {
                    VolumeData::$variant(vec![self; n])
                }
            }
        )*
    };
}

impl_volume_data_element! {
    u8 => (Uint8, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as u8),
    u16 => (Uint16, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as u16),
    u32 => (Uint32, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as u32),
    u64 => (Uint64, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as u64),
    i8 => (Int8, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as i8),
    i16 => (Int16, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as i16),
    i32 => (Int32, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as i32),
    i64 => (Int64, 0, |a, b, t| (a as f64 + (b as f64 - a as f64) * t).round() as i64),
    f32 => (Float32, 0.0, |a, b, t| a + (b - a) * t as f32),
    f64 => (Float64, 0.0, |a, b, t| a + (b - a) * t),
    Complex<f32> => (Complex64, Complex::<f32>::ZERO, |a, b, t| {
        let t = t as f32;
        Complex::new(a.re + (b.re - a.re) * t, a.im + (b.im - a.im) * t)
    }),
    Complex<f64> => (Complex128, Complex::<f64>::ZERO, |a, b, t| {
        Complex::new(a.re + (b.re - a.re) * t, a.im + (b.im - a.im) * t)
    }),
}
