"""
Seal and exit-simulation checks.

Run from research/:
    ../python/.venv/Scripts/python -m pytest bocpd_long/tests -q

"""

from __future__ import annotations

import numpy as np
import polars as pl

from bocpd_long import config
from bocpd_long import execution
from bocpd_long import minute


def test_minute_loader_never_opens_sealed_files(monkeypatch):
    opened = []
    real_read = pl.read_parquet

    def spy(source, *args, **kwargs):
        paths = source if isinstance(source, list) else [source]
        opened.extend(str(p) for p in paths)
        return real_read(source, *args, **kwargs)

    monkeypatch.setattr(minute.pl, "read_parquet", spy)
    minute.load_minutes.cache_clear()
    df = minute.load_minutes("BTCUSDT_M")
    minute.load_minutes.cache_clear()
    assert opened
    assert not any("-2026-" in p for p in opened)
    assert df["close_time"].max() < config.SEAL_START_MS
    assert not any("-2026-" in p.name for p in minute.unsealed_files("BTCUSDT"))


def _minutes(close):
    close = np.array(close, dtype=float)
    ct = np.arange(close.shape[0], dtype=np.int64) * 60_000 + 59_999
    return ct, close


def test_barriers_are_checked_at_bar_closes_only():
    # Long from 100; minute 1 closes above the profit barrier but is not a bar close
    ct, close = _minutes([100, 120, 105, 121])
    bar_ct = np.array([ct[0], ct[2], ct[3]])
    width = np.array([np.log(1.15)])
    entry, px, t, reason = execution._simulate(
        ct, close, bar_ct, np.array([ct[0]]), np.array([1]), width, 10**12
    )
    assert entry[0] == 100
    assert reason[0] == execution.PROFIT and t[0] == ct[3] and px[0] == 121


def test_loss_and_vertical_barriers():
    ct, close = _minutes([100, 99, 85, 90])
    bar_ct = ct.copy()
    width = np.array([np.log(1.15)])
    _, px, t, reason = execution._simulate(
        ct, close, bar_ct, np.array([ct[0]]), np.array([1]), width, 10**12
    )
    assert reason[0] == execution.LOSS_AT_CLOSE and px[0] == 85
    _, px, t, reason = execution._simulate(
        ct, close, bar_ct, np.array([ct[0]]), np.array([1]), width, 60_000
    )
    assert reason[0] == execution.VERTICAL and t[0] == ct[1]


def test_per_symbol_cap():
    t0 = np.array([0, 10, 20, 30], dtype=np.int64)
    texit = np.array([25, 40, 50, 60], dtype=np.int64)
    raw = np.array([0.03, 0.03, 0.03, 0.03])
    w = execution._cap_weights(t0, texit, raw, 0.05)
    assert np.allclose(w, [0.03, 0.02, 0.0, 0.03])
