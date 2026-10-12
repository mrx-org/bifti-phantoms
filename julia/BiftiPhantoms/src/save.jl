# Writes a loaded phantom back to disk: one NIfTI per property with the tissues stacked
# along the 4th dimension, plus the phantom JSON. The NIfTI bytes are deterministic
# (and the same before compression as the Python, Rust and MATLAB writers produce), so a
# saved phantom directory can be hashed.

const NIFTI_HEADER_SIZE = 348
const NIFTI_VOX_OFFSET = 352  # header plus the 4-byte (empty) extension flag
const GZIP_LEVEL = 6
const GZIP_OS_UNKNOWN = 0xff

# element type => (NIfTI datatype code, bitpix)
const NIFTI_DATATYPES = Dict{DataType,Tuple{Int16,Int16}}(
    Float64 => (64, 64),
    ComplexF64 => (1792, 128),
)

poke!(header::Vector{UInt8}, at::Integer, x) = (header[at+1:at+sizeof(x)] .= reinterpret(UInt8, [htol(x)]); header)

"""
    nifti_header(shape, n_volumes, affine, T) -> Vector{UInt8}

The 352 bytes before the voxel data of a NIfTI-1 file holding `n_volumes` volumes of
element type `T`. Orientation is stored in the sform only (`sform_code = 2`,
`qform_code = 0`), as ../../NIFTI.md requires.
"""
function nifti_header(shape, n_volumes, affine::AbstractMatrix, ::Type{T}) where {T}
    datatype, bitpix = NIFTI_DATATYPES[T]
    h = zeros(UInt8, NIFTI_VOX_OFFSET)
    poke!(h, 0, Int32(NIFTI_HEADER_SIZE))
    for (i, extent) in enumerate((4, shape..., n_volumes, 1, 1, 1))
        poke!(h, 40 + 2 * (i - 1), Int16(extent))
    end
    poke!(h, 70, datatype)
    poke!(h, 72, bitpix)
    voxel_sizes = [norm(affine[1:3, a]) for a in 1:3]
    for (i, size) in enumerate((1.0, voxel_sizes..., 1.0))  # qfac, dx, dy, dz, dt
        poke!(h, 76 + 4 * (i - 1), Float32(size))
    end
    poke!(h, 108, Float32(NIFTI_VOX_OFFSET))  # vox_offset
    poke!(h, 112, 1.0f0)                      # scl_slope
    h[124] = 2                               # xyzt_units: millimetres
    poke!(h, 252, Int16(0))                   # qform_code
    poke!(h, 254, Int16(2))                   # sform_code
    for row in 1:3, col in 1:4
        poke!(h, 280 + 16 * (row - 1) + 4 * (col - 1), Float32(affine[row, col]))
    end
    h[345:348] .= codeunits("n+1\0")
    return h
end

"""The common element type of a stack: kept if all agree, else `ComplexF64`/`Float64`."""
function stack_eltype(volumes)
    types = unique(eltype.(volumes))
    length(types) == 1 && haskey(NIFTI_DATATYPES, only(types)) && return only(types)
    return any(T -> T <: Complex, types) ? ComplexF64 : Float64
end

"""
    save_nifti(path, volumes, affine) -> Vector{Int}

Write the 3-D `volumes` (one array or a vector of them, all of one shape) as a single
4-D NIfTI-1 file, stacked along the 4th dimension, and return the zero-based index each
has in the file. Volumes with identical stored bytes share an index and are written
once. Mixed real/complex types are promoted to `ComplexF64`. A path ending in `.gz` is
gzip-compressed, with no mtime and file name so the bytes are reproducible.
"""
save_nifti(path::AbstractString, volume::AbstractArray{<:Number,3}, affine) = save_nifti(path, [volume], affine)

function save_nifti(path::AbstractString, volumes::AbstractVector, affine::AbstractMatrix)
    isempty(volumes) && throw(ArgumentError("$path: no volumes to write"))
    shape = size(first(volumes))
    all(v -> size(v) == shape, volumes) ||
        throw(ArgumentError("$path: volumes differ in shape ($(unique(size.(volumes)))), cannot stack them"))
    T = stack_eltype(volumes)

    unique_bytes = Vector{Vector{UInt8}}()
    index_of = Dict{Vector{UInt8},Int}()
    indices = map(volumes) do volume
        # Column-major is x-fastest, which is how NIfTI stores voxels.
        bytes = collect(reinterpret(UInt8, htol.(vec(convert(Array{T,3}, volume)))))
        get!(index_of, bytes) do
            push!(unique_bytes, bytes)
            length(unique_bytes) - 1
        end
    end

    blob = vcat(nifti_header(shape, length(unique_bytes), affine, T), unique_bytes...)
    if endswith(path, ".gz")
        blob = transcode(GzipCompressor(; level=GZIP_LEVEL), blob)
        blob[5:8] .= 0           # mtime
        blob[10] = GZIP_OS_UNKNOWN  # zlib writes the host's OS code
    end
    mkpath(dirname(abspath(path)))
    write(path, blob)
    return indices
end

# The per-tissue maps of a phantom and how they are grouped into files: the field of
# `BiftiTissue`/`VoxelTissue`, the file name stem, and whether it is a list of channels.
const SAVE_PROPERTIES = (
    (:density, "density", false),
    (:T1, "T1", false),
    (:T2, "T2", false),
    (:T2dash, "T2dash", false),
    (:ADC, "ADC", false),
    (:dB0, "dB0", false),
    (:B1_tx, "B1plus", true),
    (:B1_rx, "B1minus", true),
)

is_map(::NiftiRef) = true
is_map(::NiftiMapping) = true
is_map(::Float64) = false

"""
    save_bifti(dir, phantom::VoxelPhantom; json_name="phantom.json") -> String

Save a loaded phantom as a self-contained directory and return the path of its JSON.

This writes what [`load_bifti`](@ref) produced, so loading a phantom with a `reslice_to`
and saving it is how it is resliced on disk. The saved phantom has no `reslice_to` and
**no `func` mappings**: every mapping was already applied to the resampled data, and its
reference becomes a plain one.

Layout: one gzip-compressed float64 (or complex) NIfTI per property (`density.nii.gz`,
`T1.nii.gz`, `B1plus_0.nii.gz`, ...) with the tissues stacked along the 4th dimension in
order of their sorted names. Properties that are constants in the phantom stay constants
in the JSON. The output is deterministic: the same phantom always gives the same bytes.

!!! warning
    The result can be considerably **larger than the input**. Tissues that referenced the
    same map (with or without different `func`s) each get their own copy of it, unless
    the copies are bit-identical, in which case they share one volume. When reslicing,
    intensive properties are weighted by each tissue's own density, so copies of one
    source map usually differ between tissues. Data is stored as `Float64` whatever the
    type of the source NIfTIs.
"""
function save_bifti(dir::AbstractString, phantom::VoxelPhantom; json_name::AbstractString="phantom.json")
    mkpath(dir)
    names = sort!(collect(keys(phantom.config.tissues)))
    missing_names = filter(n -> !haskey(phantom.tissues, n), names)
    isempty(missing_names) || throw(ArgumentError("tissues $missing_names are in the config but were not loaded"))
    config_tissue(name) = phantom.config.tissues[name]

    # (tissue name, field, channel) => the NiftiRef the saved phantom uses instead
    refs = Dict{Tuple{String,Symbol,Int},NiftiRef}()
    for (field, stem, channels) in SAVE_PROPERTIES
        n_channels = channels ? maximum(n -> length(getfield(config_tissue(n), field)), names; init=0) : 1
        for channel in 1:n_channels
            members = filter(names) do name
                value = getfield(config_tissue(name), field)
                channels && channel > length(value) && return false
                is_map(channels ? value[channel] : value)
            end
            field === :density && (members = names)
            isempty(members) && continue

            voxels(name) = (v = getfield(phantom.tissues[name], field); channels ? v[channel] : v)
            affines = unique(phantom.tissues[n].affine for n in members)
            length(affines) == 1 ||
                throw(ArgumentError("$stem: tissues have different affines, cannot stack them into one NIfTI"))
            file = channels ? "$(stem)_$(channel - 1).nii.gz" : "$stem.nii.gz"
            indices = save_nifti(joinpath(dir, file), [voxels(n) for n in members], only(affines))
            for (name, index) in zip(members, indices)
                refs[(name, field, channel)] = NiftiRef(file, index)
            end
        end
    end

    function saved_tissue(name)
        tissue = config_tissue(name)
        ref(field, channel, value) = get(refs, (name, field, channel), value)
        property(field) = ref(field, 1, getfield(tissue, field))
        channels(field) = TissueProperty[ref(field, k, v) for (k, v) in enumerate(getfield(tissue, field))]
        return BiftiTissue(;
            density=refs[(name, :density, 1)],
            T1=property(:T1), T2=property(:T2), T2dash=property(:T2dash),
            ADC=property(:ADC), dB0=property(:dB0),
            B1_tx=channels(:B1_tx), B1_rx=channels(:B1_rx),
            unknown=tissue.unknown,
        )
    end

    config = phantom.config
    saved = BiftiPhantom(;
        schema=config.schema,
        system=config.system,
        patient=config.patient,
        reslice_to=nothing,
        tissues=OrderedDict{String,BiftiTissue}(name => saved_tissue(name) for name in names),
        unknown=config.unknown,
    )
    return write_bifti(joinpath(dir, json_name), saved)
end
