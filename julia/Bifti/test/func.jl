@testset "func expressions" begin
    x = [1.0, 2.0, 3.0, 6.0]
    # The spec's grammar: numbers in every notation, + - * /, parentheses and the x* variables.
    @test Bifti.apply_func("x - 420", x) == x .- 420
    @test Bifti.apply_func("x * 0.5 + 10", x) == x .* 0.5 .+ 10
    @test Bifti.apply_func("(x - x_min) / (x_max - x_min)", x) == (x .- 1) ./ 5
    @test Bifti.apply_func("(x - x_mean) / x_std", x) ≈ (x .- 3) ./ sqrt(3.5)
    @test Bifti.apply_func(".5 * x + 1e-3 - -1.5", x) == 0.5 .* x .+ 1e-3 .+ 1.5
    # Operators associate left and * / bind tighter than + -.
    @test Bifti.apply_func("x - 1 - 1 + 2 * 3 / 2", x) == x .+ 1
    # An expression without x still yields a map.
    @test Bifti.apply_func("2", x) == fill(2.0, 4)
    # Complex maps (B1) are transformed in the complex plane.
    @test Bifti.apply_func("x * 2", [1.0 + 1.0im]) == [2.0 + 2.0im]
    # Anything outside the grammar is rejected rather than evaluated.
    for func in ("x ^ 2", "2x", "sin(x)", "y", "x +", "(x", "x)", "run(`ls`)", "x; x")
        @test_throws ArgumentError Bifti.parse_func(func)
    end
end
