# Density-weighted footprint resampling onto a new voxel grid - the same algorithm as
# the Python and Rust implementations (see the top-level README, "Reslicing").
#
# Plain interpolation only reads the voxels around a sample point, so resampling onto a
# coarser grid throws most of the data away. Instead, each output voxel is averaged over
# the whole source region it covers. `density` is extensive and keeps a plain footprint
# average; every other property is intensive and is averaged weighted by density, so
# that the zeros outside the FOV and between tissues do not dilute it:
#
#     P_out = Σ wⱼ·Wⱼ·Pⱼ / Σ wⱼ·Wⱼ    with W = density + ε
#     D_out = Σ wⱼ·Dⱼ
#
# Taps outside the source contribute to neither sum. Where a footprint holds no tissue,
# the ε terms make the result the unweighted mean of its in-bounds taps instead of 0/0.

# Substep cap for the oblique fallback. The separable path is exact and needs no cap.
const MAX_OBLIQUE_SUBSTEPS = 8
# Relative size below which an entry counts as zero when checking axis alignment. Far
# below any real obliquity (a 1° rotation already gives ~0.017).
const AXIS_ALIGNED_TOL = 1e-9
# Weights below this (of a footprint summing to 1) are rounding crumbs, not overlap:
# inverting the source affine is inexact, so a voxel that only touches a footprint can
# get ~1e-17 instead of 0 - and then be reported as the average of an empty footprint.
const WEIGHT_EPS = 1e-12

as4x4(affine::AbstractMatrix) = size(affine) == (4, 4) ? Matrix{Float64}(affine) : [affine; 0 0 0 1]

"""
    axis_matrix(n_out, n_src, scale, offset) -> Matrix

The `(n_out, n_src)` resampling matrix of one axis, where output index `i` maps to
source coordinate `scale*i + offset` (both zero-based). Downsampling uses exact box
weights, otherwise two-tap linear interpolation. Rows sum to 1 over the unclipped
footprint, so an output voxel half outside the source keeps half its density.
"""
function axis_matrix(n_out, n_src, scale, offset)
    i = 0:n_out-1
    j = (0:n_src-1)'
    weights = if abs(scale) > 1
        lo = @. min(scale * (i - 0.5), scale * (i + 0.5)) + offset
        hi = @. max(scale * (i - 0.5), scale * (i + 0.5)) + offset
        @. max(min(hi, j + 0.5) - max(lo, j - 0.5), 0.0) / abs(scale)
    else
        @. max(1 - abs(scale * i + offset - j), 0.0)
    end
    return @. ifelse(weights < WEIGHT_EPS, 0.0, weights)
end

"""
    decompose_axis_aligned(m)

`(src_axis, scale, offset)` per output axis if the output-index → source-index affine
`m` maps every output axis onto exactly one source axis, else `nothing` (oblique).
"""
function decompose_axis_aligned(m::AbstractMatrix)
    src_axis = zeros(Int, 3)
    for a in 1:3
        column = m[1:3, a]
        norm(column) == 0 && return nothing
        s = argmax(abs.(column))
        any(abs(column[p]) > AXIS_ALIGNED_TOL * norm(column) for p in 1:3 if p != s) && return nothing
        src_axis[a] = s
    end
    isperm(src_axis) || return nothing
    return src_axis, [m[src_axis[a], a] for a in 1:3], [m[src_axis[a], 4] for a in 1:3]
end

abstract type Resampler end

"""Source and target grid are identical: resampling is a copy."""
struct IdentityResampler <: Resampler end

"""
Every output axis maps onto one source axis, so the footprint average factorizes into
three 1-D contractions and is exact. `matrices[s]` contracts source axis `s`;
`src_axis[a]` is the source axis output axis `a` comes from.
"""
struct SeparableResampler <: Resampler
    matrices::Vector{Matrix{Float64}}
    src_axis::Vector{Int}
end

"""Oblique transform: midpoint quadrature over each output voxel's parallelepiped."""
struct ObliqueResampler <: Resampler
    m::Matrix{Float64}
    substeps::NTuple{3,Int}
    shape::NTuple{3,Int}
end

"""
    Resampler(src_affine, src_shape, dst_affine, dst_shape)

Precompute the resampling from one voxel grid onto another. Affines are 3×4 or 4×4
voxel-to-world matrices with zero-based voxel indices, as in NIfTI.
"""
function Resampler(src_affine, src_shape, dst_affine, dst_shape)
    src_affine, dst_affine = as4x4(src_affine), as4x4(dst_affine)
    Tuple(src_shape) == Tuple(dst_shape) && src_affine ≈ dst_affine && return IdentityResampler()

    m = inv(src_affine) * dst_affine # output voxel index → world → source voxel index
    decomposed = decompose_axis_aligned(m)
    if isnothing(decomposed)
        span = [norm(m[1:3, a]) for a in 1:3]
        substeps = Tuple(s <= 1 ? 1 : min(MAX_OBLIQUE_SUBSTEPS, ceil(Int, s)) for s in span)
        return ObliqueResampler(m, substeps, Tuple(dst_shape))
    end
    src_axis, scale, offset = decomposed
    matrices = Vector{Matrix{Float64}}(undef, 3)
    for a in 1:3
        s = src_axis[a]
        matrices[s] = axis_matrix(dst_shape[a], src_shape[s], scale[a], offset[a])
    end
    return SeparableResampler(matrices, src_axis)
end

"""`Σ wⱼ·xⱼ` per output voxel."""
contract(::IdentityResampler, x::AbstractArray{<:Number,3}) = copy(x)

"""Contract dimension `d` of `x` with `matrix`: `y[.., i, ..] = Σⱼ matrix[i, j]·x[.., j, ..]`."""
function contract_dim(x, matrix, d)
    perm = [d; setdiff(1:3, d)]
    front = permutedims(x, perm)
    contracted = reshape(matrix * reshape(front, size(front, 1), :), size(matrix, 1), size(front)[2:3]...)
    return permutedims(contracted, invperm(perm))
end

function contract(r::SeparableResampler, x::AbstractArray{<:Number,3})
    t = foldl((t, s) -> contract_dim(t, r.matrices[s], s), 1:3; init=x)
    # Still indexed by source axis; output axis `a` is source axis `src_axis[a]`.
    return permutedims(t, r.src_axis)
end

function contract(r::ObliqueResampler, x::AbstractArray{<:Number,3})
    offsets(n) = ((1:n) .- 0.5) ./ n .- 0.5
    quadrature = Iterators.product(offsets.(r.substeps)...)
    rotation, translation = r.m[1:3, 1:3], r.m[1:3, 4]
    out = zeros(float(eltype(x)), r.shape)
    for I in CartesianIndices(out), δ in quadrature
        out[I] += trilinear(x, rotation * collect(Tuple(I) .- 1 .+ δ) + translation)
    end
    return out ./ length(quadrature)
end

"""Trilinear sample at zero-based source coordinate `c`; outside the volume reads as 0."""
function trilinear(x::AbstractArray{<:Number,3}, c)
    base = floor.(Int, c)
    frac = c .- base
    value = zero(float(eltype(x)))
    for δ in Iterators.product((0, 1), (0, 1), (0, 1))
        index = base .+ δ .+ 1
        checkbounds(Bool, x, index...) || continue
        weight = prod(ifelse.(δ .== 1, frac, 1 .- frac))
        value += weight * x[index...]
    end
    return value
end

"""
    density_weight(density)

`density + ε`, the weight intensive properties are averaged with. ε is scaled by the
density's own magnitude, since a density map need not be a [0, 1] float map.
"""
function density_weight(density)
    peak = maximum(density; init=0.0)
    return density .+ (peak > 0 ? 1e-6 * peak : 1.0)
end

"""Unweighted footprint average - the rule for the extensive `density`."""
resample_plain(r::Resampler, x) = contract(r, x)

"""Density-weighted footprint average - the rule for intensive properties."""
function resample_weighted(r::Resampler, x, weight, weight_sum)
    numerator = contract(r, x .* weight)
    return @. ifelse(weight_sum > 0, numerator / weight_sum, zero(numerator))
end
# Exact copy instead of `(x·W)/W`, which can be off by an ulp.
resample_weighted(::IdentityResampler, x, _, _) = copy(x)
