"""
Volatility estimate, symmetric CUSUM filter and triple-barrier labeling (AFML ch. 2-3).

All functions work on one symbol's bars sorted by `close_time`. A bar's values are known at
its `close_time`, so an event at bar i uses information up to and including bar i.
"""

from __future__ import annotations

import numba
import numpy as np
import polars as pl

from bocpd_long import config


def daily_vol(bars: pl.DataFrame) -> np.ndarray:
    """
    EWMA standard deviation of the log return over the last ~1 day (AFML snippet 3.1).

    For bar i the return is measured against the last bar that closed at least one day
    earlier; bars without such a reference get NaN.
    """
    log_close = np.log(bars["close"].to_numpy())
    close_time = bars["close_time"].to_numpy()
    ref = np.searchsorted(close_time, close_time - config.DAY_MS, side="right") - 1
    ret = np.where(ref >= 0, log_close - log_close[np.maximum(ref, 0)], np.nan)
    sigma = (
        pl.Series(ret)
        .fill_nan(None)
        .ewm_std(
            span=config.VOL_SPAN,
            adjust=True,
            bias=False,
            min_samples=config.VOL_MIN_BARS,
            ignore_nulls=True,
        )
        .fill_null(np.nan)
        .to_numpy()
    )
    return np.where(np.isnan(ret), np.nan, sigma)


@numba.njit(cache=True)
def cusum_events(log_close, threshold):
    """
    Symmetric CUSUM filter with a time-varying threshold (AFML snippet 2.4).

    Returns event bar indices, the trigger direction (+1 up, -1 down) and the overshoot of
    the triggering sum relative to the threshold. Bars with a NaN threshold reset the sums.
    If both sums cross on the same bar the direction follows that bar's return, so the
    filter is exactly symmetric under negating the price path.
    """
    n = log_close.shape[0]
    idx = np.empty(n, dtype=np.int64)
    direction = np.empty(n, dtype=np.int64)
    overshoot = np.empty(n, dtype=np.float64)
    k = 0
    s_pos = 0.0
    s_neg = 0.0
    for i in range(1, n):
        h = threshold[i]
        if np.isnan(h) or h <= 0.0:
            s_pos = 0.0
            s_neg = 0.0
            continue
        diff = log_close[i] - log_close[i - 1]
        s_pos = max(0.0, s_pos + diff)
        s_neg = min(0.0, s_neg + diff)
        up = s_pos > h
        down = s_neg < -h
        if up and down:
            idx[k] = i
            direction[k] = 1 if diff > 0 else -1
            overshoot[k] = (s_pos if diff > 0 else -s_neg) / h
            k += 1
            s_pos = 0.0
            s_neg = 0.0
        elif up:
            idx[k] = i
            direction[k] = 1
            overshoot[k] = s_pos / h
            k += 1
            s_pos = 0.0
        elif down:
            idx[k] = i
            direction[k] = -1
            overshoot[k] = -s_neg / h
            k += 1
            s_neg = 0.0
    return idx[:k], direction[:k], overshoot[:k]


@numba.njit(cache=True)
def triple_barrier(log_close, close_time, events, width, vertical_ms):
    """
    First touch of the horizontal barriers (+/- width) or the vertical barrier (AFML 3.4).

    Uses bar closes, as in AFML. The vertical barrier is the first bar closing at or after
    t0 + vertical_ms. Returns the touch bar index (-1 if the data ends before any barrier),
    the log return to the touch and the barrier hit (+1 upper, -1 lower, 0 vertical).
    """
    m = events.shape[0]
    n = log_close.shape[0]
    touch = np.full(m, -1, dtype=np.int64)
    ret = np.full(m, np.nan)
    barrier = np.zeros(m, dtype=np.int64)
    for e in range(m):
        i = events[e]
        w = width[e]
        t_end = close_time[i] + vertical_ms
        for j in range(i + 1, n):
            r = log_close[j] - log_close[i]
            if r >= w:
                touch[e] = j
                ret[e] = r
                barrier[e] = 1
                break
            if r <= -w:
                touch[e] = j
                ret[e] = r
                barrier[e] = -1
                break
            if close_time[j] >= t_end:
                touch[e] = j
                ret[e] = r
                barrier[e] = 0
                break
    return touch, ret, barrier


@numba.njit(cache=True)
def cusum_state(log_close, threshold):
    """
    Per-bar state of the symmetric CUSUM filter before any reset on that bar.

    Returns the dominant side (+1 / -1, 0 on a tie) and its sum relative to the threshold
    (NaN where the threshold is undefined). At CUSUM events this equals the trigger
    direction and overshoot reported by `cusum_events`.
    """
    n = log_close.shape[0]
    direction = np.zeros(n, dtype=np.int64)
    ratio = np.full(n, np.nan)
    s_pos = 0.0
    s_neg = 0.0
    for i in range(1, n):
        h = threshold[i]
        if np.isnan(h) or h <= 0.0:
            s_pos = 0.0
            s_neg = 0.0
            continue
        diff = log_close[i] - log_close[i - 1]
        s_pos = max(0.0, s_pos + diff)
        s_neg = min(0.0, s_neg + diff)
        if s_pos > -s_neg:
            direction[i] = 1
        elif s_pos < -s_neg:
            direction[i] = -1
        ratio[i] = max(s_pos, -s_neg) / h
        up = s_pos > h
        down = s_neg < -h
        if up and down:
            s_pos = 0.0
            s_neg = 0.0
        elif up:
            s_pos = 0.0
        elif down:
            s_neg = 0.0
    return direction, ratio


def label_at(bars: pl.DataFrame, idx: np.ndarray) -> pl.DataFrame:
    """
    Triple-barrier labels for events at arbitrary bar indices (any event filter).

    `cusum_dir` and `cusum_overshoot` are the CUSUM filter's state on the event bar, so the
    feature set is the same whichever filter selected the events.
    """
    log_close = np.log(bars["close"].to_numpy())
    close_time = bars["close_time"].to_numpy()
    sigma = daily_vol(bars)
    width = config.BARRIER_MULT * sigma
    idx = idx[np.isfinite(width[idx]) & (width[idx] > 0)]
    direction, ratio = cusum_state(log_close, width)
    touch, ret, barrier = triple_barrier(
        log_close,
        close_time,
        idx,
        width[idx],
        config.VERTICAL_DAYS * config.DAY_MS,
    )
    resolved = (touch >= 0) & (ret != 0.0)
    idx, touch = idx[resolved], touch[resolved]
    return pl.DataFrame(
        {
            "i0": idx,
            "i1": touch,
            "t0": close_time[idx],
            "t1": close_time[touch],
            "sigma": sigma[idx],
            "cusum_dir": direction[idx],
            "cusum_overshoot": ratio[idx],
            "ret": ret[resolved],
            "barrier": barrier[resolved],
            "bin": np.sign(ret[resolved]).astype(np.int64),
        },
    )


def label_events(bars: pl.DataFrame) -> pl.DataFrame:
    """
    Detect CUSUM events and label them with the triple barrier for one symbol.
    """
    log_close = np.log(bars["close"].to_numpy())
    close_time = bars["close_time"].to_numpy()
    sigma = daily_vol(bars)
    width = config.BARRIER_MULT * sigma

    idx, direction, overshoot = cusum_events(log_close, width)
    touch, ret, barrier = triple_barrier(
        log_close,
        close_time,
        idx,
        width[idx],
        config.VERTICAL_DAYS * config.DAY_MS,
    )
    resolved = (touch >= 0) & (ret != 0.0)
    idx, touch = idx[resolved], touch[resolved]
    return pl.DataFrame(
        {
            "i0": idx,
            "i1": touch,
            "t0": close_time[idx],
            "t1": close_time[touch],
            "sigma": sigma[idx],
            "cusum_dir": direction[resolved],
            "cusum_overshoot": overshoot[resolved],
            "ret": ret[resolved],
            "barrier": barrier[resolved],
            "bin": np.sign(ret[resolved]).astype(np.int64),
        },
    )
