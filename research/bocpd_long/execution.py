"""
Bet sizing, exit simulation on 1-minute klines and daily portfolio accounting.

Exits: each bet enters at the close of its event minute and exits at the first dollar-bar close
where the log return reaches +/- BARRIER_MULT * sigma (profit / loss) or at the first bar
closing at or after VERTICAL_DAYS (vertical). No stop orders.

Sizing: approved bets get notional MAX_SYMBOL_WEIGHT * min(1, sigma_ref / sigma) of equity,
where sigma_ref is the median sigma of real CUSUM events over the previous SIGMA_REF_DAYS days.
The summed |notional| of a symbol's open bets never exceeds MAX_SYMBOL_WEIGHT.

Accounting: a bet holds a constant quantity from entry to exit; PnL is marked at each UTC day's
last minute close and entry and exit pay the taker fee on traded notional. The daily return is
the summed PnL divided by max(1, summed time-weighted gross exposure).
"""

from __future__ import annotations

from functools import lru_cache

import numba
import numpy as np
import polars as pl
from scipy.stats import norm

from bocpd_long import bars
from bocpd_long import config
from bocpd_long import minute


VERTICAL, PROFIT, LOSS_AT_CLOSE, UNRESOLVED = 0, 1, -2, -9
MAX_SYMBOL_WEIGHT = config.MAX_SYMBOL_WEIGHT
SIGMA_REF_DAYS = config.SIGMA_REF_DAYS


def bet_size(p_meta: np.ndarray, side: np.ndarray) -> np.ndarray:
    """
    AFML bet size 2 * Phi(z) - 1 with z = (p - 0.5) / sqrt(p (1 - p)); zero when p <= 0.5.
    Only its sign (approval) is used by the strategy.
    """
    p = np.clip(p_meta, 1e-9, 1 - 1e-9)
    z = (p - 0.5) / np.sqrt(p * (1 - p))
    size = np.where(p > 0.5, 2 * norm.cdf(z) - 1, 0.0)
    return side * size


@numba.njit(cache=True)
def _simulate(m_ct, m_close, bar_ct, t0, side, width, vertical_ms):
    n_bets = t0.shape[0]
    n_min = m_ct.shape[0]
    entry = np.full(n_bets, np.nan)
    exit_px = np.full(n_bets, np.nan)
    exit_t = np.full(n_bets, -1, dtype=np.int64)
    reason = np.full(n_bets, -9, dtype=np.int64)
    for b in range(n_bets):
        j = np.searchsorted(m_ct, t0[b], side="right")
        if j == 0 or j >= n_min:
            continue
        p0 = m_close[j - 1]
        entry[b] = p0
        t_end = t0[b] + vertical_ms
        k = np.searchsorted(bar_ct, t0[b], side="right")
        while j < n_min:
            while k < bar_ct.shape[0] and bar_ct[k] < m_ct[j]:
                k += 1
            if k < bar_ct.shape[0] and bar_ct[k] == m_ct[j]:
                r = side[b] * np.log(m_close[j] / p0)
                if r >= width[b]:
                    exit_px[b] = m_close[j]
                    exit_t[b] = m_ct[j]
                    reason[b] = 1
                    break
                if r <= -width[b]:
                    exit_px[b] = m_close[j]
                    exit_t[b] = m_ct[j]
                    reason[b] = -2
                    break
                if m_ct[j] >= t_end:
                    exit_px[b] = m_close[j]
                    exit_t[b] = m_ct[j]
                    reason[b] = 0
                    break
                k += 1
            j += 1
    return entry, exit_px, exit_t, reason


def simulate_exits(symbol: str, bets: pl.DataFrame) -> pl.DataFrame:
    """
    Entry price, exit price, exit time and exit reason for one symbol's bets.
    """
    m = minute.load_minutes(symbol)
    real = symbol.removesuffix(config.MIRROR_SUFFIX)
    bar_ct = bars.load_bars(real)["close_time"].to_numpy()
    entry, exit_px, exit_t, reason = _simulate(
        m["close_time"].to_numpy(),
        m["close"].to_numpy(),
        bar_ct,
        bets["t0"].to_numpy(),
        bets["side"].to_numpy().astype(np.int64),
        config.BARRIER_MULT * bets["sigma"].to_numpy(),
        config.VERTICAL_DAYS * config.DAY_MS,
    )
    return bets.with_columns(
        pl.Series("entry_px", entry),
        pl.Series("exit_px", exit_px),
        pl.Series("exit_t", exit_t),
        pl.Series("exit_reason", reason),
    )


def sigma_reference(events: pl.DataFrame) -> pl.DataFrame:
    """
    Median sigma of real events over the trailing window, keyed by (symbol, t0).
    """
    real = events.filter(~pl.col("mirror")).select("t0", "sigma").sort("t0")
    ref = (
        real.with_columns(pl.from_epoch("t0", time_unit="ms").alias("ts"))
        .with_columns(
            pl.col("sigma")
            .rolling_median_by("ts", window_size=f"{SIGMA_REF_DAYS}d")
            .alias("sigma_ref")
        )
        .group_by("t0")
        .agg(pl.col("sigma_ref").last())
        .sort("t0")
    )
    return ref


@numba.njit(cache=True)
def _cap_weights(t0, texit, raw, cap):
    # Bets sorted by t0; open bets are tracked in a compacted list of indices
    n = t0.shape[0]
    w = np.zeros(n)
    open_idx = np.empty(n, dtype=np.int64)
    n_open = 0
    for i in range(n):
        used = 0.0
        k = 0
        for q in range(n_open):
            j = open_idx[q]
            if texit[j] > t0[i]:
                open_idx[k] = j
                k += 1
                used += abs(w[j])
        n_open = k
        w[i] = min(raw[i], max(0.0, cap - used))
        if w[i] > 0.0:
            open_idx[n_open] = i
            n_open += 1
    return w


def size_bets(bets: pl.DataFrame, sigma_ref: pl.DataFrame) -> pl.DataFrame:
    """
    Notional weight per bet with inverse-volatility scaling and the per-symbol cap.
    """
    b = (
        bets.sort("t0")
        .join_asof(sigma_ref, on="t0", strategy="backward")
        .with_columns(
            (
                MAX_SYMBOL_WEIGHT
                * pl.min_horizontal(pl.lit(1.0), pl.col("sigma_ref") / pl.col("sigma")).fill_null(
                    1.0
                )
            ).alias("w_raw"),
        )
    )
    out = []
    for _, g in b.group_by("symbol", maintain_order=True):
        g = g.sort("t0")
        t0 = g["t0"].to_numpy()
        texit = g["exit_t"].to_numpy()
        raw = g["w_raw"].to_numpy()
        w = _cap_weights(t0, texit, raw, MAX_SYMBOL_WEIGHT)
        out.append(g.with_columns(pl.Series("weight", w * g["side"].to_numpy())))
    return pl.concat(out).filter(pl.col("weight") != 0)


@lru_cache(maxsize=None)
def _daily_closes(symbol: str) -> pl.DataFrame:
    return (
        minute.load_minutes(symbol)
        .group_by((pl.col("open_time") // config.DAY_MS).alias("day"))
        .agg(pl.col("close").last().alias("day_close"))
    )


def bet_daily(bets: pl.DataFrame) -> pl.DataFrame:
    """
    Per bet and day: PnL, fees, time-weighted gross and end-of-day signed notional.
    """
    rows = []
    for (symbol,), g in bets.group_by("symbol"):
        closes = _daily_closes(symbol)
        x = (
            g.with_columns(
                (pl.col("t0") // config.DAY_MS).alias("d0"),
                (pl.col("exit_t") // config.DAY_MS).alias("d1"),
                (pl.col("weight") / pl.col("entry_px")).alias("qty"),
            )
            .with_columns(pl.int_ranges("d0", pl.col("d1") + 1).alias("day"))
            .explode("day")
            .join(closes, on="day", how="left")
            .sort("bet_id", "day")
            .with_columns(pl.col("day_close").forward_fill().over("bet_id"))
            .with_columns(
                pl.when(pl.col("day") == pl.col("d1"))
                .then(pl.col("exit_px"))
                .otherwise(pl.col("day_close"))
                .alias("px_end"),
            )
            .with_columns(
                pl.col("px_end")
                .shift(1)
                .over("bet_id")
                .fill_null(pl.col("entry_px"))
                .alias("px_start"),
                (
                    pl.min_horizontal(pl.col("exit_t"), (pl.col("day") + 1) * config.DAY_MS)
                    - pl.max_horizontal(pl.col("t0"), pl.col("day") * config.DAY_MS)
                ).alias("held_ms"),
            )
            .with_columns(
                (pl.col("qty") * (pl.col("px_end") - pl.col("px_start"))).alias("pnl"),
                (
                    config.FEE
                    * (
                        pl.when(pl.col("day") == pl.col("d0"))
                        .then(pl.col("weight").abs())
                        .otherwise(0.0)
                        + pl.when(pl.col("day") == pl.col("d1"))
                        .then((pl.col("qty") * pl.col("exit_px")).abs())
                        .otherwise(0.0)
                    )
                ).alias("fee"),
                (pl.col("weight").abs() * pl.col("held_ms") / config.DAY_MS).alias("gross"),
                pl.when(pl.col("day") < pl.col("d1"))
                .then(pl.col("qty") * pl.col("day_close"))
                .otherwise(0.0)
                .alias("notional_eod"),
                (
                    (pl.col("day") == pl.col("d0")).cast(pl.Int64)
                    + (pl.col("day") == pl.col("d1")).cast(pl.Int64)
                ).alias("orders"),
            )
        )
        rows.append(
            x.select("symbol", "bet_id", "day", "pnl", "fee", "gross", "notional_eod", "orders")
        )
    return pl.concat(rows)


def simulate_and_size(bets: pl.DataFrame, sigma_ref: pl.DataFrame) -> pl.DataFrame:
    """
    Simulate exits for approved bets (size != 0) and size them.
    """
    sims = []
    for (symbol,), g in (
        bets.filter(pl.col("size") != 0)
        .sort("symbol", "t0")
        .group_by("symbol", maintain_order=True)
    ):
        sims.append(simulate_exits(symbol, g))
        minute.load_minutes.cache_clear()
    sim = pl.concat(sims).filter(pl.col("exit_reason") != UNRESOLVED)
    return size_bets(sim, sigma_ref)


def portfolio(bets: pl.DataFrame, start_ms: int, end_ms: int) -> pl.DataFrame:
    """
    Daily portfolio returns from sized bets with simulated exits.
    """
    days = pl.DataFrame(
        {"day": np.arange(start_ms // config.DAY_MS, end_ms // config.DAY_MS, dtype=np.int64)}
    )
    per = bet_daily(bets.with_row_index("bet_id"))
    agg = per.group_by("day").agg(
        (pl.col("pnl") - pl.col("fee")).sum().alias("pnl"),
        pl.col("gross").sum(),
        pl.col("orders").sum(),
        pl.col("symbol").n_unique().alias("active_symbols"),
        pl.col("fee").sum(),
    )
    out = days.join(agg, on="day", how="left").with_columns(
        pl.col("pnl", "gross", "fee").fill_null(0.0),
        pl.col("orders", "active_symbols").fill_null(0),
    )
    return out.with_columns(
        (pl.col("pnl") / pl.max_horizontal(pl.lit(1.0), pl.col("gross"))).alias("ret"),
        (pl.col("fee") / config.FEE).alias("turnover"),
        pl.col("orders").alias("trades"),
    ).select("day", "ret", "gross", "fee", "turnover", "trades", "active_symbols")


@lru_cache(maxsize=None)
def daily_returns(symbol: str) -> pl.DataFrame:
    """
    Simple return between consecutive UTC-day final bar closes of a real symbol, keyed by day.
    """
    return (
        bars.load_bars(symbol)
        .group_by((pl.col("close_time") // config.DAY_MS).alias("day"))
        .agg(pl.col("close").last())
        .sort("day")
        .with_columns((pl.col("close") / pl.col("close").shift(1) - 1).alias("r"))
        .drop_nulls()
        .select("day", "r")
    )
