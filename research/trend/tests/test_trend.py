"""
Timing and accounting checks for the daily trend baseline.

Run from research/:
    ../python/.venv/Scripts/python -m pytest trend/tests -q

"""

from __future__ import annotations

import numpy as np
import polars as pl
import pytest

from trend import run


def _closes(prices, start=100):
    return pl.DataFrame({"day": np.arange(start, start + len(prices)), "close": prices})


def test_weights_use_only_closes_up_to_the_day():
    rng = np.random.default_rng(0)
    prices = 100 * np.exp(np.cumsum(rng.normal(0, 0.03, 200)))
    full = run.targets(_closes(prices), 13)
    cut = 150
    part = run.targets(_closes(prices[: cut - 100 + 1]), 13)
    a = full.filter(pl.col("day") <= cut).select("signal", "w").to_numpy()
    b = part.select("signal", "w").to_numpy()
    assert np.allclose(a, b, equal_nan=True)
    # The return paired with a day's weight is the following day's
    row = full.filter(pl.col("day") == cut)
    assert row["next_ret"].item() == pytest.approx(prices[cut - 100 + 1] / prices[cut - 100] - 1)


def test_signal_and_weight_cap():
    up = 100 * 1.01 ** np.arange(120)
    t = run.targets(_closes(up), 13).drop_nulls()
    assert (t["signal"] == 1.0).all()
    # A steady trend has tiny volatility, so the weight sits at the 1 / N cap
    assert np.allclose(t["w"].to_numpy(), 1 / 13)


def test_fees_follow_drifted_weights():
    w = np.array([[0.5, 0.5], [0.5, 0.5]])
    r = np.array([[0.10, -0.10], [0.0, 0.0]])
    ret, turnover, gross = run.backtest(w, r)
    assert turnover[0] == pytest.approx(1.0)
    assert ret[0] == pytest.approx(0.0 - run.FEE * 1.0)
    # After the move the book holds 0.55 / 0.45 and is rebalanced back to 0.5 / 0.5
    assert turnover[1] == pytest.approx(0.1)
    assert gross.tolist() == [1.0, 1.0]
