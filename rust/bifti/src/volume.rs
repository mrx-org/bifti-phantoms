use num_complex::Complex;

#[derive(Clone)]
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

#[derive(Clone)]
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

/// Runs `$body` (binding the inner `Vec<T>` as `$items`) against whichever variant `$self`
/// holds, so the variant list only has to be spelled out once per operation.
macro_rules! for_each_variant {
    ($self:expr, |$items:ident| $body:expr) => {
        match $self {
            VolumeData::Uint8($items) => $body,
            VolumeData::Uint16($items) => $body,
            VolumeData::Uint32($items) => $body,
            VolumeData::Uint64($items) => $body,
            VolumeData::Int8($items) => $body,
            VolumeData::Int16($items) => $body,
            VolumeData::Int32($items) => $body,
            VolumeData::Int64($items) => $body,
            VolumeData::Float32($items) => $body,
            VolumeData::Float64($items) => $body,
            VolumeData::Complex64($items) => $body,
            VolumeData::Complex128($items) => $body,
        }
    };
}

/// Like [`for_each_variant!`], but rebuilds the same variant around `$body`'s result, for
/// operations that map a volume to another volume of the same element type.
macro_rules! map_variant {
    ($self:expr, |$items:ident| $body:expr) => {
        match $self {
            VolumeData::Uint8($items) => VolumeData::Uint8($body),
            VolumeData::Uint16($items) => VolumeData::Uint16($body),
            VolumeData::Uint32($items) => VolumeData::Uint32($body),
            VolumeData::Uint64($items) => VolumeData::Uint64($body),
            VolumeData::Int8($items) => VolumeData::Int8($body),
            VolumeData::Int16($items) => VolumeData::Int16($body),
            VolumeData::Int32($items) => VolumeData::Int32($body),
            VolumeData::Int64($items) => VolumeData::Int64($body),
            VolumeData::Float32($items) => VolumeData::Float32($body),
            VolumeData::Float64($items) => VolumeData::Float64($body),
            VolumeData::Complex64($items) => VolumeData::Complex64($body),
            VolumeData::Complex128($items) => VolumeData::Complex128($body),
        }
    };
}

pub(crate) use map_variant;

impl VolumeData {
    /// Expands a single voxel volume into the given size. Panics on size != 1
    pub fn expand(self, n: usize) -> Self {
        assert_eq!(self.len(), 1);
        for_each_variant!(self, |items| items[0].fill_volume(n))
    }

    pub fn len(&self) -> usize {
        for_each_variant!(self, |items| items.len())
    }

    pub fn is_empty(&self) -> bool {
        for_each_variant!(self, |items| items.is_empty())
    }

    /// The volume's values as `f64`, for use as a resampling weight. Complex data is
    /// reduced by [`VolumeDataElement::to_f64`] (its magnitude); a `density` map is
    /// real-valued in practice, so this only matters for malformed phantoms.
    pub fn to_f64_vec(&self) -> Vec<f64> {
        for_each_variant!(self, |items| items
            .iter()
            .map(|&x| VolumeDataElement::to_f64(x))
            .collect())
    }
}

pub trait VolumeDataElement: Sized + Copy {
    const ZERO: Self;

    /// The type weighted sums are accumulated in while resampling: `f64` for the real
    /// variants, [`Complex<f64>`] for the complex ones. [`Self::to_f64`] cannot serve this
    /// role because it is lossy for complex data (it takes the magnitude).
    type Acc: Copy;

    fn into_volume(self) -> VolumeData {
        self.fill_volume(1)
    }

    /// Converts this element to a real-valued `f64`, for use in scalar mapping expressions.
    fn to_f64(x: Self) -> f64;

    /// Converts a real-valued `f64` back into this element type (the inverse of [`Self::to_f64`]).
    /// Used to bring scalar constants (literals, aggregate stats) into an expression that is
    /// otherwise evaluated entirely in the volume's native type.
    fn from_f64(x: f64) -> Self;

    /// The additive identity of [`Self::Acc`], the starting point of a weighted sum.
    fn acc_zero() -> Self::Acc;

    /// Widens an element into the accumulator type.
    fn to_acc(x: Self) -> Self::Acc;

    /// `acc + x * w` - one tap of a weighted sum. Weights are always real: they come from
    /// the geometric overlap of voxel footprints and from the (real) density map.
    fn acc_fma(acc: Self::Acc, x: Self::Acc, w: f64) -> Self::Acc;

    /// `acc / d`, for normalising a weighted sum by its total weight.
    fn acc_div(acc: Self::Acc, d: f64) -> Self::Acc;

    /// Narrows an accumulator back to the element type, rounding for integer variants.
    fn from_acc(acc: Self::Acc) -> Self;

    fn fill_volume(self, n: usize) -> VolumeData;
}

macro_rules! impl_volume_data_element {
    ($($ty:ty => (
        $variant:ident,
        $zero:expr,
        $acc:ty,
        |$x:ident| $to_f64:expr,
        |$f:ident| $from_f64:expr,
        |$t:ident| $to_acc:expr,
        |$a:ident| $from_acc:expr $(,)?
    )),* $(,)?) => {
        $(
            impl VolumeDataElement for $ty {
                const ZERO: Self = $zero;
                type Acc = $acc;

                fn to_f64($x: Self) -> f64 {
                    $to_f64
                }

                fn from_f64($f: f64) -> Self {
                    $from_f64
                }

                fn acc_zero() -> Self::Acc {
                    <$acc as Default>::default()
                }

                fn to_acc($t: Self) -> Self::Acc {
                    $to_acc
                }

                fn acc_fma(acc: Self::Acc, x: Self::Acc, w: f64) -> Self::Acc {
                    acc + x * w
                }

                fn acc_div(acc: Self::Acc, d: f64) -> Self::Acc {
                    acc / d
                }

                fn from_acc($a: Self::Acc) -> Self {
                    $from_acc
                }

                fn fill_volume(self, n: usize) -> VolumeData {
                    VolumeData::$variant(vec![self; n])
                }
            }
        )*
    };
}

impl_volume_data_element! {
    u8 => (Uint8, 0, f64, |x| x as f64, |f| f.round() as u8, |t| t as f64, |a| a.round() as u8),
    u16 => (Uint16, 0, f64, |x| x as f64, |f| f.round() as u16, |t| t as f64, |a| a.round() as u16),
    u32 => (Uint32, 0, f64, |x| x as f64, |f| f.round() as u32, |t| t as f64, |a| a.round() as u32),
    u64 => (Uint64, 0, f64, |x| x as f64, |f| f.round() as u64, |t| t as f64, |a| a.round() as u64),
    i8 => (Int8, 0, f64, |x| x as f64, |f| f.round() as i8, |t| t as f64, |a| a.round() as i8),
    i16 => (Int16, 0, f64, |x| x as f64, |f| f.round() as i16, |t| t as f64, |a| a.round() as i16),
    i32 => (Int32, 0, f64, |x| x as f64, |f| f.round() as i32, |t| t as f64, |a| a.round() as i32),
    i64 => (Int64, 0, f64, |x| x as f64, |f| f.round() as i64, |t| t as f64, |a| a.round() as i64),
    f32 => (Float32, 0.0, f64, |x| x as f64, |f| f as f32, |t| t as f64, |a| a as f32),
    f64 => (Float64, 0.0, f64, |x| x, |f| f, |t| t, |a| a),
    Complex<f32> => (Complex64, Complex::<f32>::ZERO, Complex<f64>,
        |x| x.norm() as f64,
        |f| Complex::new(f as f32, 0.0),
        |t| Complex::new(t.re as f64, t.im as f64),
        |a| Complex::new(a.re as f32, a.im as f32)),
    Complex<f64> => (Complex128, Complex::<f64>::ZERO, Complex<f64>,
        |x| x.norm(),
        |f| Complex::new(f, 0.0),
        |t| t,
        |a| a),
}
