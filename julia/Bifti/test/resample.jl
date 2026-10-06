grid_affine(spacing, origin=(0, 0, 0)) = Float64[spacing[1] 0 0 origin[1]; 0 spacing[2] 0 origin[2]; 0 0 spacing[3] origin[3]]

# Two grids sharing a FOV: the target is `factor` times coarser, so each output
# voxel covers exactly factor³ source voxels.
function coarse_grid(n, factor)
    shift = (factor - 1) / 2
    return grid_affine((factor, factor, factor), (shift, shift, shift)), ntuple(_ -> n ÷ factor, 3)
end

# With a uniform density the weighting is a no-op and the result is a plain average.
function uniform_weighted(r, x)
    weight = ones(size(x))
    return Bifti.resample_weighted(r, x, weight, Bifti.contract(r, weight))
end

block_mean(x, factor, I) = sum(x[map(i -> (factor*(i-1)+1):(factor*i), Tuple(I))...]) / factor^3

@testset "resampling" begin
    @testset "downsampling is an exact block mean" begin
        for (n, factor) in ((4, 2), (8, 4))
            x = reshape(Float64.(mod.(0:n^3-1, 7)), n, n, n)
            dst_affine, dst_shape = coarse_grid(n, factor)
            r = Bifti.Resampler(grid_affine((1, 1, 1)), size(x), dst_affine, dst_shape)
            @test r isa Bifti.SeparableResampler
            out = uniform_weighted(r, x)
            @test all(out[I] ≈ block_mean(x, factor, I) for I in CartesianIndices(out))
        end
    end

    # The reason for density weighting: a voxel straddling the FOV edge must not
    # average T1 against the zeros outside it, while density does fall off there.
    @testset "T1 is not diluted at the FOV edge" begin
        density, T1 = ones(8, 8, 8), fill(1.5, 8, 8, 8)
        r = Bifti.Resampler(grid_affine((1, 1, 1)), (8, 8, 8), grid_affine((4, 4, 4), (-2, -2, -2)), (4, 4, 4))
        weight = Bifti.density_weight(density)
        out_T1 = Bifti.resample_weighted(r, T1, weight, Bifti.contract(r, weight))
        out_density = Bifti.resample_plain(r, density)
        @test all(isapprox(t, 1.5; atol=1e-4) for (t, d) in zip(out_T1, out_density) if d > 1e-6)
        @test any(>(0.99), out_density)
        @test any(d -> 0 < d < 0.9, out_density)
    end

    # Without tissue to weight by, the result is the plain mean of the in-bounds taps.
    @testset "empty density footprint falls back to the plain mean" begin
        dst_affine, dst_shape = coarse_grid(4, 2)
        r = Bifti.Resampler(grid_affine((1, 1, 1)), (4, 4, 4), dst_affine, dst_shape)
        weight = Bifti.density_weight(zeros(4, 4, 4))
        out = Bifti.resample_weighted(r, fill(3.25, 4, 4, 4), weight, Bifti.contract(r, weight))
        @test all(≈(3.25), out)
    end

    # Axes that are not downsampled keep plain trilinear interpolation.
    @testset "upsampling matches trilinear interpolation" begin
        x = reshape(sin.(0:63), 4, 4, 4)
        src_affine, dst_affine = grid_affine((2, 2, 2)), grid_affine((1, 1, 1))
        out = uniform_weighted(Bifti.Resampler(src_affine, size(x), dst_affine, (7, 7, 7)), x)
        m = inv([src_affine; 0 0 0 1]) * [dst_affine; 0 0 0 1]
        @test all(out[I] ≈ Bifti.trilinear(x, m[1:3, 1:3] * collect(Tuple(I) .- 1) + m[1:3, 4]) for I in CartesianIndices(out))
    end

    @testset "identical grids are an exact identity" begin
        affine = grid_affine((1.5, 2, 3), (-10, 4, 0.5))
        x = reshape((0:59) .* 0.37, 3, 4, 5)
        r = Bifti.Resampler(affine, size(x), affine, size(x))
        @test r isa Bifti.IdentityResampler
        @test uniform_weighted(r, x) == x
    end

    # A rotated grid cannot separate; a constant field survives wherever it is fully covered.
    @testset "oblique grids use quadrature" begin
        c, s = 0.8, 0.6
        dst_affine = [2c -2s 0 1; 2s 2c 0 1; 0 0 2 0.5]
        r = Bifti.Resampler(grid_affine((1, 1, 1)), (8, 8, 8), dst_affine, (4, 4, 4))
        @test r isa Bifti.ObliqueResampler
        out = uniform_weighted(r, fill(2.5, 8, 8, 8))
        @test any(≈(2.5), out)
        @test all(<=(2.5 + 1e-9), out)
    end

    # B1 maps average as complex numbers: opposite phases cancel instead of reinforcing.
    @testset "complex data averages in the complex plane" begin
        x = reshape([isodd(i) ? 1.0 + 0.5im : -1.0 - 0.5im for i in 1:64], 4, 4, 4)
        dst_affine, dst_shape = coarse_grid(4, 2)
        out = uniform_weighted(Bifti.Resampler(grid_affine((1, 1, 1)), size(x), dst_affine, dst_shape), x)
        @test eltype(out) <: Complex
        @test all(v -> abs(v) < 1e-9, out)
    end

    # A voxel that only touches the source gets zero weight, not a ~1e-17 crumb that would
    # report a lone source value as the footprint average. This is the geometry of the
    # shapes_downsampled fixture.
    @testset "a voxel only touching the source gets no weight" begin
        src_affine = Float64[3 0 0 -60; 0 3 0 -48; 0 0 5 -10]
        dst_affine = Float64[9 0 0 -66; 0 9 0 -51; 0 0 10 -12.5]
        r = Bifti.Resampler(src_affine, (40, 32, 4), dst_affine, (16, 12, 3))
        weight_sum = Bifti.contract(r, ones(40, 32, 4))
        @test all(iszero, weight_sum[[1, 16], :, :])
        out = Bifti.resample_weighted(r, reshape(1.0:40*32*4, 40, 32, 4), ones(40, 32, 4), weight_sum)
        @test all(iszero, out[[1, 16], :, :])
    end
end
