//! Density-weighted footprint resampling onto a new voxel grid.
//!
//! Plain interpolation only ever reads the voxels immediately around a sample point, so
//! resampling onto a *coarser* grid throws most of the source data away and aliases. This
//! module instead averages each output voxel over the whole source region it covers.
//!
//! Averaging naively would be wrong for a phantom, though: outside the source FOV - and in
//! the background between tissues - a map reads `0`, and `T1`, `T2`, `dB0` and friends are
//! *intensive* quantities. Averaging them against those zeros drags them towards zero at
//! every edge. Only `density` is extensive. So every intensive property is averaged
//! **weighted by the tissue's density**:
//!
//! ```text
//! P_out = sum_j w_j·W_j·P_j / sum_j w_j·W_j     with W = density + eps
//! D_out = sum_j w_j·D_j
//! ```
//!
//! Sampling `W` as a volume in its own right is what makes this work: taps that fall
//! outside the source contribute nothing to *either* sum, so they are excluded rather than
//! averaged in. Where a footprint is entirely empty the `eps` terms are uniform and the
//! result falls back to the unweighted mean of the in-bounds taps, so it is never `NaN`.

use crate::{ResliceTo, volume::VolumeDataElement};

/// Substep cap for the oblique fallback. The separable path is exact and needs no cap.
const DEFAULT_MAX_SUBSTEPS: usize = 8;

/// Relative size below which a matrix entry counts as zero when checking axis alignment.
/// Far below any real obliquity (a 1 degree rotation already gives ~0.017).
const AXIS_ALIGNED_TOL: f64 = 1e-9;

/// The source-voxel taps contributing to one output voxel along one axis.
///
/// `start` is the first source index and may be out of bounds; those taps are skipped when
/// sampling, which is exactly the "outside contributes nothing" rule above. Weights sum to
/// 1 over the *unclipped* footprint, so a voxel half outside the source keeps half its
/// density and the phantom fades out at the FOV edge instead of ending abruptly.
pub(crate) struct AxisTaps {
    start: i64,
    weights: Vec<f64>,
}

/// Exact box (area) weights for one output voxel covering `[lo, hi]` in source indices.
fn box_taps(lo: f64, hi: f64) -> AxisTaps {
    let width = hi - lo;
    let first = (lo + 0.5).floor() as i64;
    let last = (hi - 0.5).ceil() as i64;
    let weights = (first..=last)
        .map(|j| {
            let j = j as f64;
            let overlap = hi.min(j + 0.5) - lo.max(j - 0.5);
            overlap.max(0.0) / width
        })
        .collect();
    AxisTaps {
        start: first,
        weights,
    }
}

/// Two-tap linear weights at source coordinate `c` - what plain trilinear interpolation
/// does, kept for axes that are not being downsampled so their behaviour is unchanged.
fn linear_taps(c: f64) -> AxisTaps {
    let j0 = c.floor();
    let f = c - j0;
    AxisTaps {
        start: j0 as i64,
        weights: vec![1.0 - f, f],
    }
}

/// Taps for every output index along one axis. `scale` and `offset` map an output index to
/// a source coordinate as `c = scale·i + offset`.
fn axis_taps(n_out: usize, scale: f64, offset: f64) -> Vec<AxisTaps> {
    let downsampling = scale.abs() > 1.0;
    (0..n_out)
        .map(|i| {
            let i = i as f64;
            if downsampling {
                let a = scale * (i - 0.5) + offset;
                let b = scale * (i + 0.5) + offset;
                box_taps(a.min(b), a.max(b))
            } else {
                linear_taps(scale * i + offset)
            }
        })
        .collect()
}

/// If `m` (output index -> source index) maps each output axis onto a single source axis,
/// returns `(source axis, scale, offset)` per output axis. `None` for oblique transforms.
fn decompose_axis_aligned(m: [[f64; 4]; 3]) -> Option<([usize; 3], [f64; 3], [f64; 3])> {
    let mut src_axis = [0usize; 3];
    let mut scale = [0.0f64; 3];
    let mut offset = [0.0f64; 3];

    for a in 0..3 {
        let col = [m[0][a], m[1][a], m[2][a]];
        let norm = (col[0] * col[0] + col[1] * col[1] + col[2] * col[2]).sqrt();
        if norm == 0.0 {
            return None; // degenerate: the output axis maps to a point
        }
        let s = (0..3).max_by(|&p, &q| col[p].abs().total_cmp(&col[q].abs()))?;
        // Every other entry of the column must be negligible for the axes to separate.
        if (0..3).any(|p| p != s && col[p].abs() > AXIS_ALIGNED_TOL * norm) {
            return None;
        }
        src_axis[a] = s;
        scale[a] = col[s];
        offset[a] = m[s][3];
    }

    // The axis assignment has to be a permutation, else two output axes share a source axis.
    let mut seen = [false; 3];
    for &s in &src_axis {
        if seen[s] {
            return None;
        }
        seen[s] = true;
    }
    Some((src_axis, scale, offset))
}

/// Per-axis substep counts for the oblique path: how finely to sample each output voxel.
fn oblique_substeps(m: [[f64; 4]; 3], max_substeps: usize) -> [usize; 3] {
    let mut steps = [1usize; 3];
    for a in 0..3 {
        let span = (m[0][a] * m[0][a] + m[1][a] * m[1][a] + m[2][a] * m[2][a]).sqrt();
        steps[a] = if span <= 1.0 {
            1
        } else {
            max_substeps.min((span.ceil() as usize).max(1))
        };
    }
    steps
}

/// A precomputed source-grid -> target-grid resampler, shared by every map of a tissue.
pub(crate) enum Resampler {
    /// Source and target grids are identical - resampling is a copy.
    Identity,
    /// Each output axis maps onto one source axis, so the footprint average factorises
    /// into three 1-D passes and is *exact*: a true box average with no quadrature error
    /// and no substep cap.
    Separable {
        /// Taps indexed by *source* axis (the pass order), each of output-axis length.
        taps: [Vec<AxisTaps>; 3],
        /// `src_axis[a]` is the source axis that output axis `a` maps onto.
        src_axis: [usize; 3],
        out_shape: [usize; 3],
    },
    /// Oblique transform: midpoint-quadrature over each output voxel's parallelepiped.
    Oblique {
        m: [[f64; 4]; 3],
        substeps: [usize; 3],
        out_shape: [usize; 3],
    },
}

impl Resampler {
    /// Build a resampler from a source grid onto the grid described by `reslice_to`.
    pub(crate) fn build(
        src_affine: [[f64; 4]; 3],
        src_shape: [usize; 3],
        reslice_to: ResliceTo,
    ) -> Self {
        let out_shape = reslice_to.resolution;
        if src_affine == reslice_to.affine && src_shape == out_shape {
            return Self::Identity;
        }

        // Output voxel index -> world -> continuous source voxel index.
        let m = crate::loader::compose_affine(
            crate::loader::invert_affine(src_affine),
            reslice_to.affine,
        );

        match decompose_axis_aligned(m) {
            Some((src_axis, scale, offset)) => {
                let mut taps: [Vec<AxisTaps>; 3] = [Vec::new(), Vec::new(), Vec::new()];
                for a in 0..3 {
                    taps[src_axis[a]] = axis_taps(out_shape[a], scale[a], offset[a]);
                }
                Self::Separable {
                    taps,
                    src_axis,
                    out_shape,
                }
            }
            None => Self::Oblique {
                m,
                substeps: oblique_substeps(m, DEFAULT_MAX_SUBSTEPS),
                out_shape,
            },
        }
    }

    /// `sum_j w_j · x_j` for every output voxel, in accumulator space.
    fn resample_acc<T: VolumeDataElement>(
        &self,
        src: &[T::Acc],
        src_shape: [usize; 3],
    ) -> Vec<T::Acc> {
        match self {
            Self::Identity => src.to_vec(),
            Self::Separable {
                taps,
                src_axis,
                out_shape,
            } => {
                // Contract one source axis at a time; the result stays indexed by source
                // axis position, so permute into output order at the end.
                let (v, shape) = contract_axis::<T>(src, src_shape, 0, &taps[0]);
                let (v, shape) = contract_axis::<T>(&v, shape, 1, &taps[1]);
                let (v, shape) = contract_axis::<T>(&v, shape, 2, &taps[2]);
                permute_to_output::<T>(&v, shape, *src_axis, *out_shape)
            }
            Self::Oblique {
                m,
                substeps,
                out_shape,
            } => oblique_resample::<T>(src, src_shape, *m, *substeps, *out_shape),
        }
    }

    /// `sum_j w_j · weight_j` per output voxel - the denominator every property of a tissue
    /// shares, so it is computed once and passed to [`Self::resample_weighted`].
    pub(crate) fn weight_sum(&self, weight: &[f64], src_shape: [usize; 3]) -> Vec<f64> {
        self.resample_acc::<f64>(weight, src_shape)
    }

    /// Unweighted footprint average - the correct rule for `density`, which is extensive.
    pub(crate) fn resample_plain<T: VolumeDataElement>(
        &self,
        data: &[T],
        src_shape: [usize; 3],
    ) -> Vec<T> {
        let src: Vec<T::Acc> = data.iter().map(|&x| T::to_acc(x)).collect();
        self.resample_acc::<T>(&src, src_shape)
            .into_iter()
            .map(T::from_acc)
            .collect()
    }

    /// Density-weighted footprint average - the correct rule for intensive properties.
    pub(crate) fn resample_weighted<T: VolumeDataElement>(
        &self,
        data: &[T],
        src_shape: [usize; 3],
        weight: &[f64],
        weight_sum: &[f64],
    ) -> Vec<T> {
        // Form weight·value in accumulator space first: for integer maps, multiplying in
        // the native type would round every tap away.
        let src: Vec<T::Acc> = data
            .iter()
            .zip(weight)
            .map(|(&x, &w)| T::acc_fma(T::acc_zero(), T::to_acc(x), w))
            .collect();
        self.resample_acc::<T>(&src, src_shape)
            .into_iter()
            .zip(weight_sum)
            .map(|(acc, &ws)| {
                if ws > 0.0 {
                    T::from_acc(T::acc_div(acc, ws))
                } else {
                    T::ZERO
                }
            })
            .collect()
    }
}

/// Contract `axis` of a 3-D array against per-output-index taps.
fn contract_axis<T: VolumeDataElement>(
    src: &[T::Acc],
    shape: [usize; 3],
    axis: usize,
    taps: &[AxisTaps],
) -> (Vec<T::Acc>, [usize; 3]) {
    let mut out_shape = shape;
    out_shape[axis] = taps.len();
    let strides = [shape[1] * shape[2], shape[2], 1];
    let mut out = vec![T::acc_zero(); out_shape[0] * out_shape[1] * out_shape[2]];

    let mut o = 0;
    for i0 in 0..out_shape[0] {
        for i1 in 0..out_shape[1] {
            for i2 in 0..out_shape[2] {
                let idx = [i0, i1, i2];
                // Base offset with the contracted axis left at 0.
                let base: usize = (0..3)
                    .filter(|&d| d != axis)
                    .map(|d| idx[d] * strides[d])
                    .sum();
                let tap = &taps[idx[axis]];
                let mut acc = T::acc_zero();
                for (k, &w) in tap.weights.iter().enumerate() {
                    let j = tap.start + k as i64;
                    if j < 0 || j >= shape[axis] as i64 {
                        continue; // outside the source: contributes to neither sum
                    }
                    acc = T::acc_fma(acc, src[base + j as usize * strides[axis]], w);
                }
                out[o] = acc;
                o += 1;
            }
        }
    }
    (out, out_shape)
}

/// Reorder an array indexed by source-axis position into output-axis order.
fn permute_to_output<T: VolumeDataElement>(
    src: &[T::Acc],
    shape: [usize; 3],
    src_axis: [usize; 3],
    out_shape: [usize; 3],
) -> Vec<T::Acc> {
    if src_axis == [0, 1, 2] {
        return src.to_vec();
    }
    let strides = [shape[1] * shape[2], shape[2], 1];
    let mut out = Vec::with_capacity(out_shape[0] * out_shape[1] * out_shape[2]);
    for o0 in 0..out_shape[0] {
        for o1 in 0..out_shape[1] {
            for o2 in 0..out_shape[2] {
                let o = [o0, o1, o2];
                let idx: usize = (0..3).map(|a| o[a] * strides[src_axis[a]]).sum();
                out.push(src[idx]);
            }
        }
    }
    out
}

/// Midpoint-quadrature footprint average for oblique transforms: sample each output voxel
/// at `substeps` evenly spaced points and interpolate the source at each.
fn oblique_resample<T: VolumeDataElement>(
    src: &[T::Acc],
    src_shape: [usize; 3],
    m: [[f64; 4]; 3],
    substeps: [usize; 3],
    out_shape: [usize; 3],
) -> Vec<T::Acc> {
    let count = (substeps[0] * substeps[1] * substeps[2]) as f64;
    let mut out = Vec::with_capacity(out_shape[0] * out_shape[1] * out_shape[2]);

    // Sub-sample offsets within an output voxel, in output index space, so the quadrature
    // follows the oblique parallelepiped the voxel actually maps to.
    let offsets =
        |n: usize| -> Vec<f64> { (0..n).map(|u| -0.5 + (u as f64 + 0.5) / n as f64).collect() };
    let (ox, oy, oz) = (
        offsets(substeps[0]),
        offsets(substeps[1]),
        offsets(substeps[2]),
    );

    for i0 in 0..out_shape[0] {
        for i1 in 0..out_shape[1] {
            for i2 in 0..out_shape[2] {
                let mut acc = T::acc_zero();
                for &dx in &ox {
                    for &dy in &oy {
                        for &dz in &oz {
                            let p = [i0 as f64 + dx, i1 as f64 + dy, i2 as f64 + dz];
                            let c = [
                                m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
                                m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
                                m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
                            ];
                            acc = T::acc_fma(acc, trilinear::<T>(src, src_shape, c), 1.0);
                        }
                    }
                }
                out.push(T::acc_div(acc, count));
            }
        }
    }
    out
}

/// Trilinear sample of an accumulator-space volume; outside the volume reads as zero.
fn trilinear<T: VolumeDataElement>(
    src: &[T::Acc],
    [nx, ny, nz]: [usize; 3],
    [x, y, z]: [f64; 3],
) -> T::Acc {
    let (x0, y0, z0) = (x.floor(), y.floor(), z.floor());
    let (fx, fy, fz) = (x - x0, y - y0, z - z0);
    let (x0, y0, z0) = (x0 as i64, y0 as i64, z0 as i64);

    let mut acc = T::acc_zero();
    for (dx, wx) in [(0, 1.0 - fx), (1, fx)] {
        for (dy, wy) in [(0, 1.0 - fy), (1, fy)] {
            for (dz, wz) in [(0, 1.0 - fz), (1, fz)] {
                let w = wx * wy * wz;
                if w == 0.0 {
                    continue;
                }
                let (xi, yi, zi) = (x0 + dx, y0 + dy, z0 + dz);
                if xi < 0
                    || yi < 0
                    || zi < 0
                    || xi >= nx as i64
                    || yi >= ny as i64
                    || zi >= nz as i64
                {
                    continue;
                }
                let idx = xi as usize * ny * nz + yi as usize * nz + zi as usize;
                acc = T::acc_fma(acc, src[idx], w);
            }
        }
    }
    acc
}

#[cfg(test)]
mod tests {
    use super::*;
    use num_complex::Complex;

    /// A 3x4 voxel-to-world affine with the given per-axis spacing and origin.
    fn affine(spacing: [f64; 3], origin: [f64; 3]) -> [[f64; 4]; 3] {
        [
            [spacing[0], 0.0, 0.0, origin[0]],
            [0.0, spacing[1], 0.0, origin[1]],
            [0.0, 0.0, spacing[2], origin[2]],
        ]
    }

    /// Two grids sharing a FOV: `src` at spacing 1, `dst` at `factor` times that. The
    /// origins line up so that each output voxel covers exactly `factor` source voxels.
    fn grids(n: usize, factor: f64) -> ([[f64; 4]; 3], [usize; 3], ResliceTo) {
        let src_affine = affine([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);
        let out = (n as f64 / factor) as usize;
        let shift = (factor - 1.0) / 2.0;
        (
            src_affine,
            [n; 3],
            ResliceTo {
                affine: affine([factor; 3], [shift, shift, shift]),
                resolution: [out; 3],
            },
        )
    }

    fn ones(n: usize) -> Vec<f64> {
        vec![1.0; n]
    }

    /// Resample `data` with a uniform density of 1 everywhere - the weighting is then a
    /// no-op and the result is a plain footprint average.
    fn weighted(r: &Resampler, data: &[f64], shape: [usize; 3]) -> Vec<f64> {
        let w = ones(data.len());
        let ws = r.weight_sum(&w, shape);
        r.resample_weighted(data, shape, &w, &ws)
    }

    #[test]
    fn downsample_2x_is_an_exact_block_mean() {
        let (src_affine, src_shape, dst) = grids(4, 2.0);
        let n = 4 * 4 * 4;
        let data: Vec<f64> = (0..n).map(|i| i as f64).collect();
        let r = Resampler::build(src_affine, src_shape, dst);
        assert!(matches!(r, Resampler::Separable { .. }));

        let out = weighted(&r, &data, src_shape);
        assert_eq!(out.len(), 8);

        // Compare against the mean of each 2x2x2 block, computed by hand.
        let at = |x: usize, y: usize, z: usize| data[x * 16 + y * 4 + z];
        for bx in 0..2 {
            for by in 0..2 {
                for bz in 0..2 {
                    let mut sum = 0.0;
                    for dx in 0..2 {
                        for dy in 0..2 {
                            for dz in 0..2 {
                                sum += at(2 * bx + dx, 2 * by + dy, 2 * bz + dz);
                            }
                        }
                    }
                    let got = out[bx * 4 + by * 2 + bz];
                    assert!(
                        (got - sum / 8.0).abs() < 1e-9,
                        "block ({bx},{by},{bz}): got {got}, want {}",
                        sum / 8.0
                    );
                }
            }
        }
    }

    #[test]
    fn downsample_4x_is_an_exact_block_mean() {
        let (src_affine, src_shape, dst) = grids(8, 4.0);
        let n = 8 * 8 * 8;
        let data: Vec<f64> = (0..n).map(|i| (i % 7) as f64).collect();
        let r = Resampler::build(src_affine, src_shape, dst);
        let out = weighted(&r, &data, src_shape);
        assert_eq!(out.len(), 8);

        let at = |x: usize, y: usize, z: usize| data[x * 64 + y * 8 + z];
        for bx in 0..2 {
            for by in 0..2 {
                for bz in 0..2 {
                    let mut sum = 0.0;
                    for dx in 0..4 {
                        for dy in 0..4 {
                            for dz in 0..4 {
                                sum += at(4 * bx + dx, 4 * by + dy, 4 * bz + dz);
                            }
                        }
                    }
                    let got = out[bx * 4 + by * 2 + bz];
                    assert!((got - sum / 64.0).abs() < 1e-9);
                }
            }
        }
    }

    /// The bug this module exists to fix: an output voxel straddling the edge of the source
    /// FOV must not average the tissue's T1 against the zeros outside it.
    #[test]
    fn t1_is_not_diluted_at_the_fov_edge() {
        // Source: 8^3 of solid tissue, density 1, T1 1.5 everywhere.
        let src_affine = affine([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);
        let src_shape = [8, 8, 8];
        let n = 8 * 8 * 8;
        let density = vec![1.0f64; n];
        let t1 = vec![1.5f64; n];

        // Target: 4x coarser and covering *more* than the source, so the outer output
        // voxels are only partly filled.
        let dst = ResliceTo {
            affine: affine([4.0, 4.0, 4.0], [-2.0, -2.0, -2.0]),
            resolution: [4, 4, 4],
        };
        let r = Resampler::build(src_affine, src_shape, dst);

        let eps = 1e-6;
        let weight: Vec<f64> = density.iter().map(|d| d + eps).collect();
        let ws = r.weight_sum(&weight, src_shape);
        let out_t1 = r.resample_weighted(&t1, src_shape, &weight, &ws);
        let out_density = r.resample_plain(&density, src_shape);

        // Every voxel that contains any tissue at all reports the tissue's true T1 ...
        for (i, (&t, &d)) in out_t1.iter().zip(&out_density).enumerate() {
            if d > 1e-6 {
                assert!(
                    (t - 1.5).abs() < 1e-4,
                    "voxel {i} has density {d} but T1 {t}, expected 1.5"
                );
            }
        }
        // ... and density *does* fall off at the edge, which is the physically correct
        // behaviour: those voxels are only partly filled with tissue.
        assert!(out_density.iter().any(|&d| d > 0.99));
        assert!(out_density.iter().any(|&d| d < 0.9 && d > 0.0));
    }

    /// A footprint with no tissue in it has no density to weight by; the result must fall
    /// back to the plain mean of the in-bounds taps rather than becoming NaN.
    #[test]
    fn empty_density_footprint_falls_back_to_plain_mean() {
        let (src_affine, src_shape, dst) = grids(4, 2.0);
        let n = 4 * 4 * 4;
        let density = vec![0.0f64; n];
        let values = vec![3.25f64; n];

        let r = Resampler::build(src_affine, src_shape, dst);
        let weight: Vec<f64> = density.iter().map(|d| d + 1.0).collect();
        let ws = r.weight_sum(&weight, src_shape);
        let out = r.resample_weighted(&values, src_shape, &weight, &ws);

        for &v in &out {
            assert!(v.is_finite(), "got a non-finite value: {v}");
            assert!((v - 3.25).abs() < 1e-9, "got {v}, want 3.25");
        }
    }

    /// Not downsampling must behave exactly as the old trilinear kernel did.
    #[test]
    fn upsample_matches_trilinear() {
        let src_affine = affine([2.0, 2.0, 2.0], [0.0, 0.0, 0.0]);
        let src_shape = [4, 4, 4];
        let n = 4 * 4 * 4;
        let data: Vec<f64> = (0..n).map(|i| (i as f64).sin()).collect();

        let dst = ResliceTo {
            affine: affine([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]),
            resolution: [7, 7, 7],
        };
        let r = Resampler::build(src_affine, src_shape, dst);
        let out = weighted(&r, &data, src_shape);

        // Reference: sample the source at each output voxel centre, trilinearly.
        let m = crate::loader::compose_affine(crate::loader::invert_affine(src_affine), dst.affine);
        let mut i = 0;
        for x in 0..7 {
            for y in 0..7 {
                for z in 0..7 {
                    let p = [x as f64, y as f64, z as f64];
                    let c = [
                        m[0][0] * p[0] + m[0][1] * p[1] + m[0][2] * p[2] + m[0][3],
                        m[1][0] * p[0] + m[1][1] * p[1] + m[1][2] * p[2] + m[1][3],
                        m[2][0] * p[0] + m[2][1] * p[1] + m[2][2] * p[2] + m[2][3],
                    ];
                    let want = trilinear::<f64>(&data, src_shape, c);
                    assert!(
                        (out[i] - want).abs() < 1e-9,
                        "voxel {i}: {} vs {want}",
                        out[i]
                    );
                    i += 1;
                }
            }
        }
    }

    #[test]
    fn identity_grid_is_an_exact_identity() {
        let a = affine([1.5, 2.0, 3.0], [-10.0, 4.0, 0.5]);
        let shape = [3, 4, 5];
        let n = 3 * 4 * 5;
        let data: Vec<f64> = (0..n).map(|i| i as f64 * 0.37).collect();

        let r = Resampler::build(
            a,
            shape,
            ResliceTo {
                affine: a,
                resolution: shape,
            },
        );
        assert!(matches!(r, Resampler::Identity));
        assert_eq!(weighted(&r, &data, shape), data);
    }

    /// A rotated target grid cannot separate, and must take the quadrature path.
    #[test]
    fn oblique_affine_uses_the_quadrature_path() {
        let src_affine = affine([1.0, 1.0, 1.0], [0.0, 0.0, 0.0]);
        let src_shape = [8, 8, 8];
        let (c, s) = (0.8f64, 0.6f64); // a 37 degree rotation about z
        let dst = ResliceTo {
            affine: [
                [2.0 * c, -2.0 * s, 0.0, 1.0],
                [2.0 * s, 2.0 * c, 0.0, 1.0],
                [0.0, 0.0, 2.0, 0.5],
            ],
            resolution: [4, 4, 4],
        };
        let r = Resampler::build(src_affine, src_shape, dst);
        assert!(matches!(r, Resampler::Oblique { .. }));

        // A constant field must survive resampling unchanged wherever it is fully covered.
        let data = vec![2.5f64; 8 * 8 * 8];
        let out = weighted(&r, &data, src_shape);
        assert!(out.iter().all(|v| v.is_finite()));
        assert!(out.iter().any(|v| (v - 2.5).abs() < 1e-9));
        assert!(out.iter().all(|&v| v <= 2.5 + 1e-9));
    }

    /// Complex maps (`B1+`/`B1-`) must average as complex numbers, not magnitudes: two
    /// opposite phases average to zero, they do not reinforce.
    #[test]
    fn complex_data_averages_in_the_complex_plane() {
        let (src_affine, src_shape, dst) = grids(4, 2.0);
        let n = 4 * 4 * 4;
        // Alternate +1 and -1 along z, so every 2x2x2 block cancels exactly.
        let data: Vec<Complex<f64>> = (0..n)
            .map(|i| {
                if i % 2 == 0 {
                    Complex::new(1.0, 0.5)
                } else {
                    Complex::new(-1.0, -0.5)
                }
            })
            .collect();

        let r = Resampler::build(src_affine, src_shape, dst);
        let w = ones(n);
        let ws = r.weight_sum(&w, src_shape);
        let out = r.resample_weighted(&data, src_shape, &w, &ws);

        for v in &out {
            assert!(v.norm() < 1e-9, "expected cancellation, got {v}");
        }
    }

    /// Integer maps accumulate in f64 and are rounded once at the end.
    #[test]
    fn integer_maps_round_after_averaging() {
        let (src_affine, src_shape, dst) = grids(2, 2.0);
        // A single 2x2x2 block holding seven 0s and one 4 -> mean 0.5 -> rounds to 1.
        let mut data = vec![0u8; 8];
        data[0] = 4;

        let r = Resampler::build(src_affine, src_shape, dst);
        let w = ones(8);
        let ws = r.weight_sum(&w, src_shape);
        let out = r.resample_weighted(&data, src_shape, &w, &ws);
        assert_eq!(out, vec![1u8]);
    }
}
