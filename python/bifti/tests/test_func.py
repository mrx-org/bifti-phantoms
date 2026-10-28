from __future__ import annotations

import numpy as np
import pytest

from bifti.loader import eval_expr, parse_func

x = np.array([1.0, 2.0, 3.0, 6.0])


@pytest.mark.parametrize(
    "func, expected",
    [
        # The spec's grammar: numbers in every notation, + - * /, parentheses and the x* variables.
        ("x - 420", x - 420),
        ("x * 0.5 + 10", x * 0.5 + 10),
        ("(x - x_min) / (x_max - x_min)", (x - 1) / 5),
        ("(x - x_mean) / x_std", (x - 3) / np.sqrt(3.5)),
        (".5 * x + 1e-3 - -1.5", 0.5 * x + 1e-3 + 1.5),
        # Operators associate left and * / bind tighter than + -.
        ("x - 1 - 1 + 2 * 3 / 2", x + 1),
        # An expression without x still yields a map.
        ("2", np.full(4, 2.0)),
    ],
)
def test_func_follows_the_grammar(func, expected):
    np.testing.assert_allclose(eval_expr(func, x), expected)


def test_complex_maps_are_transformed_in_the_complex_plane():
    np.testing.assert_array_equal(eval_expr("x * 2", np.array([1.0 + 1.0j])), [2.0 + 2.0j])


@pytest.mark.parametrize(
    "func",
    ["x ^ 2", "x ** 2", "2x", "sin(x)", "y", "x +", "(x", "x)", "x; x", "0x10", "1j",
     "().__class__.__base__.__subclasses__()", "__import__('os').system('ls')"],
)
def test_anything_outside_the_grammar_is_rejected(func):
    with pytest.raises(ValueError):
        parse_func(func)
