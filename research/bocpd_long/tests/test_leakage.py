"""
Leakage and symmetry checks.

Run from research/:
    ../python/.venv/Scripts/python -m pytest bocpd_long/tests -q

"""

from __future__ import annotations

import numpy as np
import polars as pl
import pytest

from bocpd_long import bars
from bocpd_long import bocpd
from bocpd_long import config
from bocpd_long import cv
from bocpd_long import events
from bocpd_long import features


def _synthetic_bars(n: int = 3000, seed: int = 0) -> pl.DataFrame:
    rng = np.random.default_rng(seed)
    close = 100 * np.exp(np.cumsum(rng.normal(0, 0.01, n)))
    open_ = np.concatenate([[100.0], close[:-1]])
    spread = np.abs(rng.normal(0, 0.004, n))
    high = np.maximum(open_, close) * (1 + spread)
    low = np.minimum(open_, close) * (1 - spread)
    close_time = np.arange(n, dtype=np.int64) * 7_200_000 + 7_199_999
    qv = np.full(n, 1e6)
    vol = qv / close
    tb = rng.uniform(0.3, 0.7, n)
    return pl.DataFrame(
        {
            "open_time": close_time - 7_199_999,
            "close_time": close_time,
            "open": open_,
            "high": high,
            "low": low,
            "close": close,
            "volume": vol,
            "quote_volume": qv,
            "taker_buy_volume": vol * tb,
            "taker_buy_quote_volume": qv * tb,
            "n_minutes": rng.integers(30, 240, n),
        },
    )


def test_bars_never_read_sealed_period():
    for symbol in bars.real_symbols()[:5]:
        assert bars.load_bars(symbol)["close_time"].max() < config.SEAL_START_MS


def test_mirror_events_are_symmetric():
    df = _synthetic_bars()
    real = events.label_events(df)
    mirror = events.label_events(bars.mirror_bars(df))
    assert real.height == mirror.height > 0
    assert (real["i0"] == mirror["i0"]).all()
    assert (real["i1"] == mirror["i1"]).all()
    assert (real["cusum_dir"] == -mirror["cusum_dir"]).all()
    assert (real["bin"] == -mirror["bin"]).all()


@pytest.mark.parametrize("cut", [1500, 2200])
def test_features_use_only_past_bars(cut):
    # Changing bars after the event bar must not change that event's features
    df = _synthetic_bars()
    ev = events.label_events(df).filter(pl.col("i0") <= cut).tail(5)
    sigma = events.daily_vol(df)
    base = features.event_features(df, sigma, 0.15, ev)

    rng = np.random.default_rng(1)
    future = df["close"].to_numpy().copy()
    future[cut + 1 :] *= np.exp(rng.normal(0, 0.05, future.shape[0] - cut - 1))
    shocked = df.with_columns(pl.Series("close", future))
    shocked_sigma = events.daily_vol(shocked)
    after = features.event_features(shocked, shocked_sigma, 0.15, ev)

    cols = [c for c in features.FEATURES if c != "sigma"]
    assert np.allclose(
        base.select(cols).to_numpy().astype(float),
        after.select(cols).to_numpy().astype(float),
        equal_nan=True,
    )
    assert np.allclose(sigma[: cut + 1], shocked_sigma[: cut + 1], equal_nan=True)


def test_purged_kfold_has_no_overlap_and_respects_embargo():
    rng = np.random.default_rng(3)
    t0 = np.sort(rng.integers(0, 1_000_000, 2000))
    t1 = t0 + rng.integers(1, 20_000, 2000)
    embargo = 5_000
    folds = cv.purged_kfold(t0, t1, 5, embargo)
    seen = np.concatenate([te for _, te in folds])
    assert np.array_equal(np.sort(seen), np.arange(t0.size))
    for train, test in folds:
        start, end = t0[test].min(), t1[test].max()
        assert not np.any((t0[train] <= end) & (t1[train] >= start))
        assert not np.any((t0[train] > end) & (t0[train] <= end + embargo))


def test_same_timestamp_events_share_a_fold():
    t0 = np.repeat(np.arange(1000, dtype=np.int64) * 10, 2)
    groups = cv.time_groups(t0, 5)
    assert np.all(groups[0::2] == groups[1::2])


def test_cpcv_paths_cover_each_group_once():
    rng = np.random.default_rng(4)
    t0 = np.sort(rng.integers(0, 1_000_000, 3000))
    t1 = t0 + 100
    splits, groups, combos = cv.cpcv(t0, t1, 6, 2, 0)
    paths = cv.cpcv_paths(6, combos)
    assert paths.shape == (5, 6)
    for p in range(paths.shape[0]):
        for g in range(6):
            assert g in combos[paths[p, g]]
    for train, test in splits:
        assert np.intersect1d(train, test).size == 0


def test_bocpd_triggers_are_identical_on_the_mirror():
    df = _synthetic_bars()
    assert np.array_equal(
        bocpd.trigger_index(df["close"].to_numpy()),
        bocpd.trigger_index(bars.mirror_bars(df)["close"].to_numpy()),
    )
