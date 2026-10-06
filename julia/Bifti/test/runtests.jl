using Bifti
using LinearAlgebra: det
using Test

# The example phantoms shared with the Python and Rust test suites.
const DATA = joinpath(@__DIR__, "..", "..", "..", "python", "bifti", "examples", "data")

@testset "Bifti" begin
    include("phantom.jl")
    include("func.jl")
    include("resample.jl")
    include("loader.jl")
    include("registry.jl")
end
