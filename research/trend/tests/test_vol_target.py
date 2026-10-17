"""
Timing checks for the OI confirmation and the volatility cap.

Run from research/:
    ../python/.venv/Scripts/python -m pytest trend/tests -q

"""

from __future__ import annotations

import numpy as np
import polars as pl
import pytest

from trend import common
from trend import run
from trend import vol_target


DAY = common.DAY_MS
STEP = vol_target.STEP_MS


def test_oi_level_is_assigned_to_the_day_it_is_known():
    d = 20_000
    metrics = pl.DataFrame(
        {
            # Stamped 23:50 and 23:55 of day d (known by 00:00), then 00:00 of day d + 1
            "time": [(d + 1) * DAY - 2 * STEP, (d + 1) * DAY - STEP, (d + 1) * DAY],
            "sum_open_interest": [1.0, 2.0, 3.0],
        }
    )
    out = vol_target.oi_by_day(metrics)
    assert out.rows() == [(d, 2.0), (d + 1, 3.0)]


def test_oi_factor_scales_weights_between_floor_and_one():
    n = max(run.LOOKBACKS) + 5
    days = np.arange(100, 100 + n)
    targets = pl.DataFrame({"day": days, "w": np.full(n, 0.05)})
    rising = pl.DataFrame({"day": days, "oi": 1.0 + np.arange(n, dtype=float)})
    falling = pl.DataFrame({"day": days, "oi": 1000.0 - np.arange(n, dtype=float)})
    up = vol_target.with_oi_weights(targets, rising).drop_nulls("oi_signal")
    down = vol_target.with_oi_weights(targets, falling).drop_nulls("oi_signal")
    assert np.allclose(up["w"].to_numpy(), 0.05)
    assert np.allclose(down["w"].to_numpy(), 0.05 * (2 * vol_target.OI_FLOOR - 1))
    # No signal before the longest lookback has history
    assert up.height == n - max(run.LOOKBACKS)


def test_vol_cap_uses_only_past_returns_and_never_levers_up():
    rng = np.random.default_rng(1)
    n_days, k = 200, 3
    r = rng.normal(0, 0.05, (n_days, k))
    w = np.full((n_days, k), 0.3)
    scale = vol_target.vol_cap_scale(w, r)
    i = 150
    r_changed = r.copy()
    r_changed[i:] *= 10
    assert vol_target.vol_cap_scale(w, r_changed)[i] == pytest.approx(scale[i])
    assert np.isnan(scale[: vol_target.COV_MIN_DAYS]).all()
    s = scale[~np.isnan(scale)]
    assert (s <= 1.0).all() and (s < 1.0).any()
    # Where the cap binds, the scaled book sits exactly at the target
    past = r[i - vol_target.COV_DAYS : i]
    vol = np.sqrt(w[i] @ np.cov(past, rowvar=False) @ w[i] * 365)
    assert vol * scale[i] == pytest.approx(min(vol, vol_target.TARGET_VOL))
    # A quiet book is left alone
    assert np.allclose(vol_target.vol_cap_scale(w * 1e-3, r)[~np.isnan(scale)], 1.0)
