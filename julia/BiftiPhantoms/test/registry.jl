const REPO = joinpath(@__DIR__, "..", "..", "..")
file_url(name) = "file://" * abspath(joinpath(REPO, name))

@testset "registry" begin
    catalog = load_catalog(file_url("catalog.json"))
    registry = load_registry(file_url("registry.json"))
    # Every catalog label resolves to a registry collection listing phantom JSONs.
    for collection in values(catalog)
        @test haskey(registry, collection)
        phantoms = flatten_phantoms(registry[collection])
        @test !isempty(phantoms) && all(endswith(".json"), phantoms)
    end

    # Groups nest arbitrarily and are flattened depth-first.
    nested = Any["a.json", Dict("group" => "g", "phantoms" => Any["b.json", Dict("group" => "h", "phantoms" => ["c.json"])]), "d.json"]
    @test flatten_phantoms(nested) == ["a.json", "b.json", "c.json", "d.json"]

    @test BiftiPhantoms.zenodo_record_id("10.5281/zenodo.20384437") == "20384437"
    @test_throws ArgumentError BiftiPhantoms.zenodo_record_id("10.1000/xyz")
    @test BiftiPhantoms.escape_uri("subj42_B1+ v2.nii.gz") == "subj42_B1%2B%20v2.nii.gz"

    # Downloads from Zenodo only when explicitly enabled, so the suite runs offline.
    if get(ENV, "BIFTI_TEST_NETWORK", "false") == "true"
        cache_dir = mktempdir()
        path = load_registry_phantom("endres-bifti_demo-001", "shapes.json"; registry, cache_dir)
        @test load_bifti(path).config.tissues == read_bifti(joinpath(DATA, "shapes.json")).tissues
    end
end
