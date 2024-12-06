import numpy as np
import pytest

from nautilus_trader.common.math import quadratic_interpolation


class TestQuadraticInterpolation:
    @pytest.fixture
    def xs(self):
        return np.array(
            [
                0.08333333,
                0.16666667,
                0.25,
                0.33333333,
                0.5,
                1.0,
                2.0,
                3.0,
                5.0,
                7.0,
                10.0,
                20.0,
                30.0,
            ],
        )

    @pytest.fixture
    def ys(self):
        return np.array(
            [
                0.0459,
                0.0453,
                0.0446,
                0.0446,
                0.0438,
                0.0423,
                0.0415,
                0.041,
                0.0407,
                0.0412,
                0.0417,
                0.0443,
                0.0433,
            ],
        )

    def test_below(self, xs, ys):
        assert quadratic_interpolation(0.01, xs, ys) == pytest.approx(0.0459)

    def test_above(self, xs, ys):
        assert quadratic_interpolation(40.0, xs, ys) == pytest.approx(0.0433)

    def test_at_point(self, xs, ys):
        assert quadratic_interpolation(10.0, xs, ys) == pytest.approx(0.0417)

    def test_interpolation(self, xs, ys):
        assert quadratic_interpolation(0.75, xs, ys) == pytest.approx(0.0429, abs=1e-4)
