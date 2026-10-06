"""
    Bifti

Read, write and load [BIfTI phantoms](https://github.com/mrx-org/bifti-phantoms): a
JSON file defining tissues and their MR properties, referencing NIfTI files for the
per-voxel data.

- [`read_bifti`](@ref) / [`write_bifti`](@ref): the phantom JSON as a [`BiftiPhantom`](@ref)
- [`load_bifti`](@ref): the phantom with all NIfTI data, as a [`VoxelPhantom`](@ref)
- [`load_catalog`](@ref), [`load_registry`](@ref), [`load_registry_phantom`](@ref): the public registry
"""
module Bifti

using Downloads: Downloads
using JSON: JSON
using LinearAlgebra: I, inv, norm
using NIfTI: niread
using OrderedCollections: OrderedDict
using Scratch: @get_scratch!
using Statistics: mean, std
using Tar: Tar

include("phantom.jl")
include("func.jl")
include("resample.jl")
include("loader.jl")
include("registry.jl")

export BiftiPhantom, BiftiTissue, NiftiRef, NiftiMapping, PhantomSystem, Patient, PatientPosition, ResliceTo
export read_bifti, write_bifti, nifti_files, scanner_matrix, scanner_affine
export VoxelPhantom, VoxelTissue, load_bifti
export load_catalog, load_registry, load_registry_phantom, flatten_phantoms

end # module Bifti
