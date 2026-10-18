"""
Fill-rule and accounting checks for the passive execution diagnostic.

Run from research/:
    ../python/.venv/Scripts/python -m pytest trend/tests -q

"""

from __future__ import annotations

import numpy as np
import pytest

from trend import passive
from trend import run


def _bars(low, high, close):
    return {
        k: np.array([v], dtype=float) for k, v in (("low", low), ("high", high), ("close", close))
    }


def test_buy_fills_only_when_the_market_trades_through():
    p0 = np.array([100.0])
    # Interval 0 (minutes 0-1) trades through 100; interval 1 only touches its limit of 100
    bars = _bars([99.9, 100.0, 100.0, 100.0], [100.2] * 4, [100.0, 100.0, 100.0, 101.0])
    cost, share = passive.unit_costs("passive", p0, bars, slices=2)[1]
    assert share[0] == pytest.approx(0.5)
    # Half filled at P0 as maker, half bought at the window's last price as taker
    expected = 0.5 * passive.MAKER_FEE + 0.5 * (101.0 / 100.0 - 1 + passive.TAKER_FEE)
    assert cost[0] == pytest.approx(expected)


def test_unfilled_quantity_rests_and_is_repegged():
    p0 = np.array([100.0])
    # Nothing in interval 0; interval 1 re-pegs at the last price 99 and a high of 99.5 fills
    # every released unit of a sell
    bars = _bars([99.0] * 4, [100.0, 99.0, 99.5, 99.2], [99.5, 99.0, 99.2, 99.1])
    cost, share = passive.unit_costs("passive", p0, bars, slices=2)[-1]
    assert share[0] == pytest.approx(1.0)
    assert cost[0] == pytest.approx(-(99.0 / 100.0 - 1) + passive.MAKER_FEE)


def test_baseline_reproduces_the_backtest():
    rng = np.random.default_rng(2)
    w = rng.uniform(-0.1, 0.1, (50, 4))
    r = rng.normal(0, 0.03, (50, 4))
    fee = np.full((50, 4), run.FEE)
    zero = np.zeros((50, 4))
    ret, share, unit = passive.book(w, r, fee, fee, zero, zero)
    assert np.allclose(ret, run.backtest(w, r)[0])
    assert share == 0.0
    assert unit == pytest.approx(run.FEE)
