import numpy as np

from nautilus_trader.core.stats import fast_mean
from nautilus_trader.core.stats import fast_std


def test_np_mean(benchmark):
    benchmark(
        np.mean,
        np.random.default_rng(10).random(100),
    )


def test_np_std(benchmark):
    benchmark(np.std, np.random.default_rng(10).random(100))


def test_fast_mean(benchmark):
    benchmark(fast_mean, np.random.default_rng(10).random(100))


def test_fast_std(benchmark):
    benchmark(fast_std, np.random.default_rng(10).random(100))
