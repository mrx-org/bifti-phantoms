# Turns a parsed phantom config into arrays: reads the referenced NIfTIs, resamples
# them onto one grid per tissue and applies `func` mappings.

"""
    VoxelTissue

One tissue with every property resolved to an array on the same 3-D grid. Scalar
properties are expanded, so uniform and spatially varying tissues look alike.
`B1_tx`/`B1_rx` hold one array per channel, complex if the NIfTI data is complex.

Units follow ../../JSON.md: `T1`, `T2`, `T2dash` in s, `ADC` in 10⁻³ mm²/s, `dB0` in
Hz, `B1_tx`/`B1_rx` relative, `density` a volume fraction. `affine` is the 3×4
voxel-to-world (RAS+, mm) matrix for zero-based voxel indices, as in NIfTI.
"""
struct VoxelTissue{TX<:Number,RX<:Number}
    density::Array{Float64,3}
    T1::Array{Float64,3}
    T2::Array{Float64,3}
    T2dash::Array{Float64,3}
    ADC::Array{Float64,3}
    dB0::Array{Float64,3}
    B1_tx::Vector{Array{TX,3}}
    B1_rx::Vector{Array{RX,3}}
    affine::Matrix{Float64}
end

Base.size(tissue::VoxelTissue) = size(tissue.density)
Base.show(io::IO, tissue::VoxelTissue) = print(io, "VoxelTissue(", join(size(tissue), "×"),
    ", ", length(tissue.B1_tx), " B1+ / ", length(tissue.B1_rx), " B1- channels)")

"""
    VoxelPhantom

A loaded phantom: the parsed `config` ([`BiftiPhantom`](@ref)) and its `tissues`
as [`VoxelTissue`](@ref)s, in the order of the JSON file.
"""
struct VoxelPhantom
    config::BiftiPhantom
    tissues::OrderedDict{String,VoxelTissue}
end

Base.show(io::IO, phantom::VoxelPhantom) = print(io, "VoxelPhantom(", join(keys(phantom.tissues), ", "), ")")

"""
    load_bifti(path) -> VoxelPhantom
    load_bifti(config::BiftiPhantom, base_dir) -> VoxelPhantom

Load a phantom JSON and every NIfTI it references (resolved relative to the JSON's
directory). Each tissue is brought onto `reslice_to` if the phantom has one, otherwise
onto the grid of its own density map - which, for a conforming phantom, every map
already shares. Pass an already parsed `config` to tweak it before loading.
"""
load_bifti(path::AbstractString) = load_bifti(read_bifti(path), dirname(abspath(path)))

function load_bifti(config::BiftiPhantom, base_dir::AbstractString)
    # One NIfTI usually holds a property for several tissues, so read each file once.
    cache = Dict{String,Tuple{Array,Matrix{Float64}}}()
    read_ref(ref::NiftiRef) = read_subvolume(cache, base_dir, ref)
    tissues = OrderedDict{String,VoxelTissue}(
        name => load_tissue(tissue, read_ref, config.reslice_to) for (name, tissue) in config.tissues
    )
    return VoxelPhantom(config, tissues)
end

"""Everything a tissue's maps share: the target grid and the density weighting."""
struct TissueGrid{R<:Resampler}
    resampler::R
    target::ResliceTo
    src_affine::Matrix{Float64}
    src_shape::NTuple{3,Int}
    weight::Array{Float64,3}
    weight_sum::Array{Float64,3}
end

function TissueGrid(density, src_affine, target::ResliceTo)
    resampler = Resampler(src_affine, size(density), target.affine, target.resolution)
    weight = density_weight(density)
    return TissueGrid(resampler, target, src_affine, size(density), weight, contract(resampler, weight))
end

function load_tissue(tissue::BiftiTissue, read_ref, reslice_to)
    density_src, src_affine = read_ref(tissue.density)
    real_valued(density_src, "density")
    target = something(reslice_to, ResliceTo(src_affine, size(density_src)))
    grid = TissueGrid(density_src, src_affine, target)

    property(p, name) = real_valued(load_property(p, read_ref, grid, name), name)
    channels(ps, name) = promote_channels([load_property(p, read_ref, grid, "$name[$(i-1)]") for (i, p) in enumerate(ps)])
    return VoxelTissue(
        # `density` is extensive, so it is averaged unweighted: weighting it by itself
        # would remove exactly the partial-volume information resampling should produce.
        resample_plain(grid.resampler, density_src),
        property(tissue.T1, "T1"),
        property(tissue.T2, "T2"),
        property(tissue.T2dash, "T2'"),
        property(tissue.ADC, "ADC"),
        property(tissue.dB0, "dB0"),
        channels(tissue.B1_tx, "B1+"),
        channels(tissue.B1_rx, "B1-"),
        target.affine,
    )
end

"""Resolve one scalar-or-map property onto the tissue's target grid."""
load_property(value::Float64, _, grid::TissueGrid, _) = fill(value, grid.target.resolution)
load_property(ref::NiftiRef, read_ref, grid::TissueGrid, name) = resample(grid, read_ref(ref)..., name)
# `func` and its `x_*` statistics act on the resampled values, as in Python and Rust.
load_property(mapping::NiftiMapping, read_ref, grid::TissueGrid, name) =
    apply_func(mapping.func, load_property(mapping.file, read_ref, grid, name))

function resample(grid::TissueGrid, data, affine, name)
    size(data) == grid.src_shape && affine ≈ grid.src_affine &&
        return resample_weighted(grid.resampler, data, grid.weight, grid.weight_sum)
    # ../../NIFTI.md requires all NIfTIs of a phantom to share one grid. A non-conforming
    # map cannot use the density weight, so it falls back to an unweighted average.
    @warn "$name does not share the density map's grid; resampling it unweighted, so its values may be diluted near edges"
    target = grid.target
    return resample_plain(Resampler(affine, size(data), target.affine, target.resolution), data)
end

function real_valued(data, name)
    eltype(data) <: Real || throw(ArgumentError("$name must be real-valued, got $(eltype(data)) data"))
    return data
end

# Channels of one B1 array share a type: complex if any channel is.
promote_channels(channels) = convert(Vector{Array{mapreduce(eltype, promote_type, channels),3}}, channels)

"""
    read_subvolume(cache, base_dir, ref) -> (data, affine)

The sub-volume `ref` points to on its native grid, as `Float64` (or `ComplexF64` for
complex NIfTIs), plus the file's 3×4 sform affine.
"""
function read_subvolume(cache, base_dir, ref::NiftiRef)
    path = isabspath(ref.file) ? ref.file : normpath(joinpath(base_dir, ref.file))
    data, affine = get!(() -> read_nifti(path), cache, path)
    0 <= ref.index < size(data, 4) ||
        throw(BoundsError("$(ref.file) has $(size(data, 4)) sub-volumes, cannot read [$(ref.index)]"))
    return data[:, :, :, ref.index+1], affine
end

function read_nifti(path)
    volume = niread(path)
    ndims(volume.raw) == 4 || throw(ArgumentError("$path must be 4-dimensional (../../NIFTI.md), got size $(size(volume.raw))"))
    eltype(volume.raw) <: Union{Real,Complex} || throw(ArgumentError("$path has unsupported data type $(eltype(volume.raw))"))
    header = volume.header
    data = float.(volume.raw)
    header.scl_slope == 0 || (data = data .* header.scl_slope .+ header.scl_inter)
    affine = Float64[collect(header.srow_x)'; collect(header.srow_y)'; collect(header.srow_z)']
    return convert(Array{eltype(data) <: Complex ? ComplexF64 : Float64}, data), affine
end

"""
    scanner_affine(affine, patient_or_position) -> Matrix
    scanner_affine(phantom::VoxelPhantom, tissue) -> Matrix

Compose a 3×4 or 4×4 voxel → RAS+ affine with a patient position, giving the 4×4
voxel → scanner affine `P₄ · A` (../../NIFTI.md#patient-position). `nothing` is `FFS`,
which leaves the affine unchanged.
"""
function scanner_affine(affine::AbstractMatrix, position::PatientPosition)
    rotation = Matrix{Float64}(I, 4, 4)
    rotation[1:3, 1:3] = scanner_matrix(position)
    return rotation * as4x4(affine)
end

scanner_affine(affine::AbstractMatrix, patient::Patient) = scanner_affine(affine, patient.position)
scanner_affine(affine::AbstractMatrix, ::Nothing) = scanner_affine(affine, FFS)
scanner_affine(phantom::VoxelPhantom, tissue::AbstractString) =
    scanner_affine(phantom.tissues[tissue].affine, phantom.config.patient)
