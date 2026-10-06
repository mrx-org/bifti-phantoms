load_example(name) = load_bifti(joinpath(DATA, "$name.json"))

@testset "loading" begin
    native = load_example("shapes")

    @testset "without reslice_to, tissues keep the density grid" begin
        for tissue in values(native.tissues)
            @test size(tissue) == (40, 32, 4)
            @test all(size(map) == (40, 32, 4) for map in (tissue.T1, tissue.dB0, tissue.B1_tx...))
        end
        # Scalars are expanded, defaults filled in and `func` applied voxel-wise.
        @test all(==(0.6), native.tissues["ring"].T1)
        @test all(==(Inf), native.tissues["background"].T2dash)
        @test native.tissues["ring"].dB0 ≈ native.tissues["disk"].dB0 .* 0.5 .+ 10
        @test length(native.tissues["disk"].B1_tx) == 2
        @test collect(keys(native.tissues)) == ["disk", "ring", "background"]
    end

    @testset "reslice_to brings every map onto the target grid" begin
        for (name, shape) in (("shapes_resliced", (60, 48, 4)), ("shapes_downsampled", (16, 12, 3)))
            phantom = load_example(name)
            for tissue in values(phantom.tissues)
                @test all(size(map) == shape for map in (tissue.density, tissue.T1, tissue.dB0, tissue.B1_tx...))
                @test all(isfinite, tissue.density) && all(isfinite, tissue.T1)
                @test tissue.affine == phantom.config.reslice_to.affine
            end
        end
    end

    # A density-weighted average is a convex combination of source values, so it can
    # never leave their range - averaging against background zeros would.
    @testset "resampling never invents values outside the source range" begin
        for name in ("shapes_resliced", "shapes_downsampled"), (tissue_name, tissue) in load_example(name).tissues
            for property in (:T1, :T2, :dB0)
                source = getfield(native.tissues[tissue_name], property)
                @test minimum(source) - 1e-6 <= minimum(getfield(tissue, property))
                @test maximum(getfield(tissue, property)) <= maximum(source) + 1e-6
            end
        end
    end

    voxel_volume(tissue) = abs(prod(tissue.affine[i, i] for i in 1:3))

    # Density is extensive: its integral over the FOV is invariant under resampling.
    @testset "downsampling conserves the total tissue amount" begin
        for (name, tissue) in load_example("shapes_downsampled").tissues
            fine = native.tissues[name]
            @test sum(tissue.density) * voxel_volume(tissue) ≈ sum(fine.density) * voxel_volume(fine) rtol = 0.02
        end
    end

    # The background is uniform plus independent noise, and each interior output voxel
    # averages 3×3×2 = 18 source voxels, so the noise must drop by about √18.
    @testset "downsampling averages instead of point-sampling" begin
        fine = native.tissues["background"].density
        interior = load_example("shapes_downsampled").tissues["background"].density[3:end-2, 3:end-2, 2:end-1]
        @test sum(interior) / length(interior) ≈ sum(fine) / length(fine) rtol = 0.02
        std(x) = sqrt(sum(abs2, x .- sum(x) / length(x)) / length(x))
        @test std(interior) ≈ std(fine) / sqrt(18) rtol = 0.25
    end

    @testset "multi-channel B1+ and a func mapping on a resliced grid" begin
        phantom = load_example("subj42-3T")
        for tissue in values(phantom.tissues)
            @test size(tissue) == (100, 100, 1)
            @test all(size(ch) == (100, 100, 1) for ch in tissue.B1_tx)
            @test all(isfinite, tissue.dB0)
        end
        @test length(phantom.tissues["gm"].B1_tx) == 8
        # fat's dB0 is the shared dB0 map shifted by the fat-water offset.
        @test phantom.tissues["fat"].dB0 ≈ phantom.tissues["wm"].dB0 .- 420 rtol = 1e-2
    end

    @testset "scanner affine follows the patient position" begin
        affine = native.tissues["disk"].affine
        @test native.config.patient == Patient(Bifti.HFS)
        @test scanner_affine(native, "disk") == [scanner_matrix(Bifti.HFS) * affine; 0 0 0 1]
        # Without a patient position (FFS) the affine is unchanged.
        @test scanner_affine(affine, nothing) == [affine; 0 0 0 1]
    end

    @testset "invalid references fail loudly" begin
        config = read_bifti(joinpath(DATA, "shapes.json"))
        tissue = config.tissues["disk"]
        broken = BiftiPhantom(; tissues=Bifti.OrderedDict("disk" => BiftiTissue(; density=NiftiRef(tissue.density.file, 3))))
        @test_throws ArgumentError load_bifti(broken, DATA)
    end
end
