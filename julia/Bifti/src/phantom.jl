# The phantom JSON data model (../../JSON.md): parsing, validation and serialization.
# No NIfTI I/O happens here - see loader.jl for turning a config into arrays.

const DEFAULT_SCHEMA = "https://raw.githubusercontent.com/mrx-org/bifti-phantoms/refs/heads/main/bifti-phantom-v1.schema.json"
# Any URI ending in bifti-phantom-v1 (or the older nifti-phantom-v1) is accepted;
# underscores match the legacy plain `file_type` value.
const SCHEMA_REGEX = r"(nifti|bifti)[-_]phantom[-_]v1(\.[^/]*)?$"
const NIFTI_REF_REGEX = r"^(?<file>.+?)\[(?<index>\d+)\]$"

# Units are fixed in v1 so that readers never have to convert; the field only
# makes a file self-documenting.
const UNITS = OrderedDict(
    "gyro" => "MHz/T", "B0" => "T", "T1" => "s", "T2" => "s", "T2'" => "s",
    "ADC" => "10^-3 mm^2/s", "dB0" => "Hz", "B1+" => "rel", "B1-" => "rel",
)

# The format is additively extensible (../../SPEC.md), so unknown fields are ignored
# rather than rejected - but the schema no longer catches typos, which leaves the
# reader as the only place that can point one out.
function warn_unknown_fields(where, config, known)
    for key in keys(config)
        key in known || @warn "Ignoring unknown field $(repr(key)) in $where"
    end
end

unknown_fields(config, known) =
    OrderedDict{String,Any}(k => v for (k, v) in config if !(k in known))

"""
    PatientPosition

DICOM-style patient position code (`FFS`, `FFP`, `FFDR`, `FFDL`, `HFS`, `HFP`,
`HFDR`, `HFDL`). Each defines a rotation from phantom RAS+ into scanner
coordinates, see [`scanner_matrix`](@ref).
"""
@enum PatientPosition FFS FFP FFDR FFDL HFS HFP HFDR HFDL

# Scanner frame: right-handed, Z along B0 pointing out of the bore, Y up - which
# makes FFS the identity (../../NIFTI.md#patient-position).
const SCANNER_MATRICES = Dict(
    FFS => [1 0 0; 0 1 0; 0 0 1],
    FFP => [-1 0 0; 0 -1 0; 0 0 1],
    FFDR => [0 1 0; -1 0 0; 0 0 1],
    FFDL => [0 -1 0; 1 0 0; 0 0 1],
    HFS => [-1 0 0; 0 1 0; 0 0 -1],
    HFP => [1 0 0; 0 -1 0; 0 0 -1],
    HFDR => [0 -1 0; -1 0 0; 0 0 -1],
    HFDL => [0 1 0; 1 0 0; 0 0 -1],
)

function Base.parse(::Type{PatientPosition}, code::AbstractString)
    for position in instances(PatientPosition)
        string(position) == code && return position
    end
    throw(ArgumentError("Unknown patient position $(repr(code)), expected one of $(join(instances(PatientPosition), ", "))"))
end

"""
    scanner_matrix(position::PatientPosition)
    scanner_matrix(phantom)

The 3×3 rotation `P` from phantom RAS+ into scanner coordinates,
`v_scanner = P * v_ras`. A phantom without `patient` is `FFS`, the identity.
"""
scanner_matrix(position::PatientPosition) = Float64.(SCANNER_MATRICES[position])

"""How the subject lies in the scanner (`patient` in ../../JSON.md)."""
struct Patient
    position::PatientPosition
end

function Patient(config::AbstractDict)
    warn_unknown_fields("patient", config, ("position",))
    return Patient(parse(PatientPosition, config["position"]))
end

to_dict(patient::Patient) = OrderedDict{String,Any}("position" => string(patient.position))

"""Global MR system parameters: `gyro` [MHz/T] and `B0` [T]."""
Base.@kwdef struct PhantomSystem
    gyro::Float64 = 42.5764
    B0::Float64 = 3.0
end

function PhantomSystem(config::AbstractDict)
    warn_unknown_fields("system", config, ("gyro", "B0"))
    return PhantomSystem(config["gyro"], config["B0"])
end

to_dict(system::PhantomSystem) = OrderedDict{String,Any}("gyro" => system.gyro, "B0" => system.B0)

"""
    NiftiRef(file, index)

Reference to sub-volume `index` (zero-based, along the 4th dimension) of a NIfTI
`file` stored next to the phantom JSON. Written as `"file.nii.gz[index]"`.
"""
struct NiftiRef
    file::String
    index::Int
end

function Base.parse(::Type{NiftiRef}, s::AbstractString)
    m = match(NIFTI_REF_REGEX, s)
    isnothing(m) && throw(ArgumentError("Invalid NIfTI reference $(repr(s)), expected \"<file>[<index>]\""))
    return NiftiRef(m[:file], parse(Int, m[:index]))
end

Base.print(io::IO, ref::NiftiRef) = print(io, ref.file, "[", ref.index, "]")

"""
    NiftiMapping(file::NiftiRef, func)

A NIfTI reference whose voxel values are remapped by the expression `func`
(see ../../JSON.md, "Transformed reference").
"""
struct NiftiMapping
    file::NiftiRef
    func::String
end

function NiftiMapping(config::AbstractDict)
    warn_unknown_fields("a transformed reference", config, ("file", "func"))
    return NiftiMapping(parse(NiftiRef, config["file"]), config["func"])
end

to_dict(mapping::NiftiMapping) = OrderedDict{String,Any}("file" => string(mapping.file), "func" => mapping.func)

"""A scalar-or-map property: a number, a [`NiftiRef`](@ref) or a [`NiftiMapping`](@ref)."""
const TissueProperty = Union{Float64,NiftiRef,NiftiMapping}

parse_property(value::Real) = Float64(value)
parse_property(value::AbstractString) = parse(NiftiRef, value)
parse_property(value::AbstractDict) = NiftiMapping(value)

serialize_property(value::Float64) = value
serialize_property(ref::NiftiRef) = string(ref)
serialize_property(mapping::NiftiMapping) = to_dict(mapping)

const TISSUE_FIELDS = ("density", "T1", "T2", "T2'", "ADC", "dB0", "B1+", "B1-")

"""
    BiftiTissue(; density, T1=Inf, T2=Inf, T2dash=Inf, ADC=0.0, dB0=0.0, B1_tx=[1.0], B1_rx=[1.0])

One tissue of a phantom config. Every property except `density` is a
[`TissueProperty`](@ref); `B1_tx`/`B1_rx` hold one per channel. Fields the
reader does not know are kept in `unknown`, so saving round-trips them.
"""
Base.@kwdef struct BiftiTissue
    density::NiftiRef
    T1::TissueProperty = Inf
    T2::TissueProperty = Inf
    T2dash::TissueProperty = Inf
    ADC::TissueProperty = 0.0
    dB0::TissueProperty = 0.0
    B1_tx::Vector{TissueProperty} = TissueProperty[1.0]
    B1_rx::Vector{TissueProperty} = TissueProperty[1.0]
    unknown::OrderedDict{String,Any} = OrderedDict{String,Any}()
end

function BiftiTissue(config::AbstractDict)
    property(key, default) = parse_property(get(config, key, default))
    channels(key) = TissueProperty[parse_property(ch) for ch in get(config, key, [1.0])]
    return BiftiTissue(;
        density=parse(NiftiRef, config["density"]),
        T1=property("T1", Inf),
        T2=property("T2", Inf),
        T2dash=property("T2'", Inf),
        ADC=property("ADC", 0.0),
        dB0=property("dB0", 0.0),
        B1_tx=channels("B1+"),
        B1_rx=channels("B1-"),
        unknown=unknown_fields(config, TISSUE_FIELDS),
    )
end

function to_dict(tissue::BiftiTissue)
    config = OrderedDict{String,Any}("density" => string(tissue.density))
    # Defaults are omitted - `Inf` could not be written as JSON anyway.
    for (key, value, default) in (
        ("T1", tissue.T1, Inf), ("T2", tissue.T2, Inf), ("T2'", tissue.T2dash, Inf),
        ("ADC", tissue.ADC, 0.0), ("dB0", tissue.dB0, 0.0),
    )
        isequal(value, default) || (config[key] = serialize_property(value))
    end
    for (key, channels) in (("B1+", tissue.B1_tx), ("B1-", tissue.B1_rx))
        isequal(channels, TissueProperty[1.0]) || (config[key] = serialize_property.(channels))
    end
    return merge(config, tissue.unknown)
end

"""
    ResliceTo(affine, resolution)

Target grid every NIfTI is resampled onto: the upper 3×4 rows of the
voxel-to-world `affine` [mm] and the matrix size `resolution`.
"""
struct ResliceTo
    affine::Matrix{Float64}
    resolution::NTuple{3,Int}
    function ResliceTo(affine::AbstractMatrix, resolution)
        size(affine) == (3, 4) || throw(ArgumentError("reslice_to affine must be 3×4, got $(size(affine))"))
        return new(affine, Tuple(resolution))
    end
end

function ResliceTo(config::AbstractDict)
    warn_unknown_fields("reslice_to", config, ("affine", "resolution"))
    affine = permutedims(reduce(hcat, [Float64.(row) for row in config["affine"]]))
    return ResliceTo(affine, Int.(config["resolution"]))
end

Base.:(==)(a::ResliceTo, b::ResliceTo) = a.affine == b.affine && a.resolution == b.resolution

to_dict(reslice::ResliceTo) = OrderedDict{String,Any}(
    "affine" => [collect(row) for row in eachrow(reslice.affine)],
    "resolution" => collect(reslice.resolution),
)

const PHANTOM_FIELDS = ("\$schema", "units", "system", "patient", "reslice_to", "tissues")

"""
    BiftiPhantom(; system=PhantomSystem(), tissues=OrderedDict(), patient=nothing, reslice_to=nothing)

The parsed phantom JSON (units, system, tissue definitions) without any voxel
data. Read and write it with [`read_bifti`](@ref) / [`write_bifti`](@ref);
load the referenced NIfTIs with [`load_bifti`](@ref).

`patient === nothing` means `FFS`: an unpositioned phantom is never transformed.
"""
Base.@kwdef struct BiftiPhantom
    schema::String = DEFAULT_SCHEMA
    system::PhantomSystem = PhantomSystem()
    patient::Union{Nothing,Patient} = nothing
    reslice_to::Union{Nothing,ResliceTo} = nothing
    tissues::OrderedDict{String,BiftiTissue} = OrderedDict{String,BiftiTissue}()
    unknown::OrderedDict{String,Any} = OrderedDict{String,Any}()
end

function BiftiPhantom(config::AbstractDict)
    schema = get(config, "\$schema") do
        get(config, "file_type") do
            throw(ArgumentError("Phantom has no \$schema"))
        end
    end
    occursin(SCHEMA_REGEX, schema) || throw(ArgumentError("Unsupported \$schema: $(repr(schema))"))
    # Only the default units are supported for now, mirroring the Python and Rust readers.
    Dict(config["units"]) == Dict(UNITS) || throw(ArgumentError("Only the default units are supported, got $(config["units"])"))

    warn_unknown_fields("the phantom", config, (PHANTOM_FIELDS..., "file_type"))
    for (name, tissue) in config["tissues"]
        warn_unknown_fields("tissue $(repr(name))", tissue, TISSUE_FIELDS)
    end

    return BiftiPhantom(;
        schema,
        system=PhantomSystem(config["system"]),
        patient=haskey(config, "patient") ? Patient(config["patient"]) : nothing,
        reslice_to=haskey(config, "reslice_to") ? ResliceTo(config["reslice_to"]) : nothing,
        tissues=OrderedDict{String,BiftiTissue}(name => BiftiTissue(t) for (name, t) in config["tissues"]),
        unknown=unknown_fields(config, (PHANTOM_FIELDS..., "file_type")),
    )
end

function to_dict(phantom::BiftiPhantom)
    config = OrderedDict{String,Any}(
        "\$schema" => phantom.schema,
        "units" => UNITS,
        "system" => to_dict(phantom.system),
    )
    isnothing(phantom.patient) || (config["patient"] = to_dict(phantom.patient))
    isnothing(phantom.reslice_to) || (config["reslice_to"] = to_dict(phantom.reslice_to))
    config["tissues"] = OrderedDict(name => to_dict(t) for (name, t) in phantom.tissues)
    return merge(config, phantom.unknown)
end

scanner_matrix(phantom::BiftiPhantom) =
    scanner_matrix(isnothing(phantom.patient) ? FFS : phantom.patient.position)

"""
    read_bifti(path) -> BiftiPhantom

Parse a phantom JSON file without loading any NIfTI data.
"""
read_bifti(path::AbstractString) = BiftiPhantom(JSON.parsefile(path))

"""
    write_bifti(path, phantom::BiftiPhantom)

Serialize a phantom config to JSON, creating the parent directory if needed.
"""
function write_bifti(path::AbstractString, phantom::BiftiPhantom)
    mkpath(dirname(abspath(path)))
    open(io -> JSON.json(io, to_dict(phantom); pretty=2), path, "w")
    return path
end

"""
    nifti_files(phantom::BiftiPhantom) -> Vector{String}

Every distinct NIfTI file referenced across all tissues, in order of first use.
"""
function nifti_files(phantom::BiftiPhantom)
    files = String[]
    add!(ref::NiftiRef) = ref.file in files || push!(files, ref.file)
    add!(mapping::NiftiMapping) = add!(mapping.file)
    add!(::Float64) = nothing
    for tissue in values(phantom.tissues)
        foreach(add!, (tissue.density, tissue.T1, tissue.T2, tissue.T2dash, tissue.ADC, tissue.dB0))
        foreach(add!, tissue.B1_tx)
        foreach(add!, tissue.B1_rx)
    end
    return files
end
