using NIfTI: niread

function same_maps(a, b)
    # a constant stays a constant, so it may reload as a single voxel
    length(b) == 1 || length(a) == 1 ? all(==(first(b)), a) || all(==(first(a)), b) : a == b
end

function same_phantoms(a, b)
    keys(a.tissues) == keys(b.tissues) || return false
    all(keys(a.tissues)) do name
        ta, tb = a.tissues[name], b.tissues[name]
        all(f -> same_maps(getfield(ta, f), getfield(tb, f)), (:density, :T1, :T2, :T2dash, :ADC, :dB0)) &&
            length(ta.B1_tx) == length(tb.B1_tx) && all(same_maps.(ta.B1_tx, tb.B1_tx)) &&
            length(ta.B1_rx) == length(tb.B1_rx) && all(same_maps.(ta.B1_rx, tb.B1_rx))
    end
end

read_dir(dir) = Dict(f => read(joinpath(dir, f)) for f in readdir(dir))

@testset "saving" begin
    @testset "round trip: $name" for name in ("shapes", "shapes_resliced", "subj42-3T")
        original = load_example(name)
        mktempdir() do dir
            json = save_bifti(dir, original)
            @test json == joinpath(dir, "phantom.json")
            saved = read_bifti(json)
            @test saved.reslice_to === nothing
            # func is baked in
            @test !any(t -> any(p -> p isa NiftiMapping, (t.T1, t.T2, t.T2dash, t.ADC, t.dB0)), values(saved.tissues))
            @test same_phantoms(original, load_bifti(json))
        end
    end

    @testset "constants stay constants" begin
        mktempdir() do dir
            saved = read_bifti(save_bifti(dir, load_example("shapes")))
            @test saved.tissues["disk"].T1 == 1.0
            @test !isfile(joinpath(dir, "T1.nii.gz"))
        end
    end

    @testset "stack layout and header" begin
        original = load_example("shapes")
        mktempdir() do dir
            save_bifti(dir, original)
            volume = niread(joinpath(dir, "density.nii.gz"))
            @test size(volume.raw) == (40, 32, 4, 3)
            @test volume.header.sform_code == 2 && volume.header.qform_code == 0
            # tissues are stacked in sorted-name order: background, disk, ring
            @test volume.raw[:, :, :, 2] == original.tissues["disk"].density
            header = volume.header
            @test Float64[collect(header.srow_x)'; collect(header.srow_y)'; collect(header.srow_z)'] ≈
                  original.tissues["disk"].affine
        end
    end

    @testset "deterministic output" begin
        original = load_example("shapes_resliced")
        reversed = VoxelPhantom(original.config, BiftiPhantoms.OrderedDict(reverse(collect(original.tissues))))
        mktempdir() do a
            mktempdir() do b
                save_bifti(a, original)
                save_bifti(b, reversed)
                @test read_dir(a) == read_dir(b)
                header = read(joinpath(a, "density.nii.gz"), 10)
                @test header[1:2] == [0x1f, 0x8b]
                @test header[4] == 0              # no file name
                @test all(==(0), header[5:8])     # no mtime
                @test header[10] == 0xff          # OS unknown
            end
        end
    end

    @testset "identical maps share one volume" begin
        config = read_bifti(joinpath(DATA, "shapes.json"))
        config.tissues["ring2"] = config.tissues["ring"]
        phantom = load_bifti(config, DATA)
        mktempdir() do dir
            saved = read_bifti(save_bifti(dir, phantom))
            @test saved.tissues["ring"].dB0 == saved.tissues["ring2"].dB0
            @test saved.tissues["ring"].density == saved.tissues["ring2"].density
            @test size(niread(joinpath(dir, "dB0.nii.gz")).raw, 4) == 2  # disk + ring, not 3
        end
    end
end
