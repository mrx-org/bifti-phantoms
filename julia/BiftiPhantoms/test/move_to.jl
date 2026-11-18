# `reslice_to.move_to`: placing the resampled grid independently of the sampling.

@testset "move_to" begin
    sampled = [3.0 0 0 -50; 0 2.0 0 7; 0 0 5.0 12]
    position(affine, voxel) = affine[:, 1:3] * voxel + affine[:, 4]
    with_move_to(config, move_to) = BiftiPhantom(; config.schema, config.system, config.patient, config.tissues,
        reslice_to=ResliceTo(config.reslice_to.affine, config.reslice_to.resolution; move_to))

    @testset "without move_to the grid stays where it was sampled" begin
        r = ResliceTo(sampled, (4, 5, 1))
        @test placed_affine(r) == sampled
        @test !haskey(BiftiPhantoms.to_dict(r), "move_to")
    end

    @testset "center puts voxel n ÷ 2 at the origin (n = $n)" for n in (4, 5)
        r = ResliceTo(sampled, (n, n, 1); move_to=:center)
        placed = placed_affine(r)
        @test placed[:, 1:3] == sampled[:, 1:3]
        # even: -n/2 .. n/2-1, odd: -(n-1)/2 .. (n-1)/2
        @test [position(placed, [i, 0, 0])[1] / 3 for i in 0:n-1] == (0:n-1) .- n ÷ 2
    end

    @testset "center keeps rotation" begin
        c, s = 0.8, 0.6
        affine = [2c -2s 0 10; 2s 2c 0 -4; 0 0 3 1]
        r = ResliceTo(affine, (6, 4, 2); move_to="center")
        @test r.move_to === :center
        @test all(abs.(position(placed_affine(r), [3, 2, 1])) .< 1e-12)
    end

    @testset "explicit move_to is used as given and round-trips" begin
        target = [3.0 0 0 1; 0 2.0 0 2; 0 0 5.0 3; 0 0 0 1]
        r = ResliceTo(sampled, (4, 5, 1); move_to=target)
        @test placed_affine(r) == target[1:3, :]
        @test ResliceTo(BiftiPhantoms.to_dict(r)) == r
        centered = ResliceTo(sampled, (4, 5, 1); move_to=:center)
        @test ResliceTo(BiftiPhantoms.to_dict(centered)) == centered
    end

    @testset "invalid move_to is rejected" begin
        @test_throws ArgumentError ResliceTo(sampled, (4, 5, 1); move_to="middle")
        @test_throws ArgumentError ResliceTo(sampled, (4, 5, 1); move_to=[1.0 0 0; 0 1 0])
    end

    @testset "different voxel size warns" begin
        @test_logs (:warn, r"stretched") ResliceTo(sampled, (4, 5, 1); move_to=[1.0 0 0 0; 0 2 0 0; 0 0 5 0])
    end

    @testset "move_to changes placement, not sampling" begin
        config = read_bifti(joinpath(DATA, "shapes_resliced.json"))
        plain = load_bifti(config, DATA)
        moved_config = with_move_to(config, :center)
        moved = load_bifti(moved_config, DATA)
        for (name, tissue) in moved.tissues
            @test tissue.density == plain.tissues[name].density
            @test tissue.T1 == plain.tissues[name].T1
            @test tissue.affine == placed_affine(moved_config.reslice_to)
        end
    end

    @testset "saved phantom keeps the placed affine" begin
        config = with_move_to(read_bifti(joinpath(DATA, "shapes_resliced.json")), :center)
        mktempdir() do dir
            json = save_bifti(dir, load_bifti(config, DATA))
            reloaded = load_bifti(json)
            @test reloaded.config.reslice_to === nothing
            for tissue in values(reloaded.tissues)
                @test isapprox(tissue.affine, placed_affine(config.reslice_to); atol=1e-4)
            end
        end
    end
end
