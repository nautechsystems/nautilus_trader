"""
Trials 2 and 3 of the daily trend book: open-interest confirmation, then a portfolio volatility
cap. Rebuilt on 2026-10-17 from the recorded rules after the original files were lost.

Both start from the trial 1 weights in `run.py` (same universe, closes, signal and costs):

- Trial 2 (OI confirmation): weight * (OI_FLOOR + (1 - OI_FLOOR) * oi_signal), where oi_signal
  is the mean over run.LOOKBACKS of the sign of the log change in open interest (contracts).
  A metrics row stamped t covers [t, t + 5 min) and its open interest is the level at t + 5 min,
  so the level used at a day's 00:00 UTC close is the row stamped 23:55 (the day's last row).
- Trial 3 (volatility cap): the trial 2 weights scaled by min(1, TARGET_VOL / ex-ante vol),
  where ex-ante vol = sqrt(w' S w * 365) and S is the covariance of the 13 coins' daily simple
  returns over the COV_DAYS days before the close. The book is scaled down only, never levered
  up. TARGET_VOL = 20% was chosen near trial 2's realized volatility, a mild look-ahead.

Both trials are registered (the deflated Sharpe ratio counts every trial evaluated).

Run from `research/`: `../python/.venv/Scripts/python -m trend.vol_target`.
"""

from __future__ import annotations

import re
from pathlib import Path

import numpy as np
import polars as pl

from trend import common
from trend import run


TRIAL_OI = f"{run.TRIAL}:oi_confirm_floor0.75"
TRIAL_CAP = f"{TRIAL_OI}:vol_cap20_cov60"
METRICS_DIR = common.ROOT / "binance_data" / "futures_metrics"
OUT_DIR_OI = common.ROOT / "binance_data" / "trend_oi"
OUT_DIR_CAP = common.ROOT / "binance_data" / "trend_vol_target"
STEP_MS = 300_000
OI_FLOOR = 0.75
TARGET_VOL = 0.20
COV_DAYS = 60
COV_MIN_DAYS = 30
_METRICS_RE = re.compile(r"-metrics-(\d{4})-(\d{2})\.parquet$")


def unsealed_metrics(symbol: str) -> list[Path]:
    """
    Monthly metrics files before the seal month, selected by name before anything is read.
    """
    out = []
    for path in sorted((METRICS_DIR / symbol).glob("*.parquet")):
        m = _METRICS_RE.search(path.name)
        if m is None:
            raise ValueError(f"Unexpected file name: {path}")
        if (int(m[1]), int(m[2])) < common.SEAL_MONTH:
            out.append(path)
    return out


def oi_by_day(metrics: pl.DataFrame) -> pl.DataFrame:
    """
    Open interest known at each UTC day's closing boundary, keyed by `day`.

    `metrics` has `time` (row stamp, ms) and `sum_open_interest`.

    """
    end = pl.col("time") + STEP_MS  # when the row's level is known
    return (
        metrics.drop_nulls(["time", "sum_open_interest"])
        .filter(end <= common.SEAL_START_MS)
        .group_by(((end - 1) // common.DAY_MS).alias("day"))
        .agg(pl.col("sum_open_interest").sort_by("time").last().alias("oi"))
        .sort("day")
    )


def with_oi_weights(targets: pl.DataFrame, oi: pl.DataFrame) -> pl.DataFrame:
    """
    Add oi_signal and scale the trial 1 weight `w` by the OI confirmation factor.
    """
    lo = pl.col("oi").log()
    signal = pl.mean_horizontal([(lo - lo.shift(n)).sign() for n in run.LOOKBACKS])
    return (
        targets.join(oi, on="day", how="left")
        .sort("day")
        .with_columns(pl.col("oi").forward_fill())
        .with_columns(
            # mean_horizontal skips nulls; require every lookback
            pl.when(lo.shift(max(run.LOOKBACKS)).is_not_null()).then(signal).alias("oi_signal"),
        )
        .with_columns(
            (pl.col("w") * (OI_FLOOR + (1 - OI_FLOOR) * pl.col("oi_signal"))).alias("w"),
        )
    )


def vol_cap_scale(w: np.ndarray, r: np.ndarray) -> np.ndarray:
    """
    Per row, min(1, TARGET_VOL / ex-ante annual vol of the weights w[i]).

    Row i holds the weights set at close i and r[i], the returns realized over the next day, so
    only r[:i] is known at close i. Rows without enough history or weights get NaN.

    """
    scale = np.full(w.shape[0], np.nan)
    for i in range(w.shape[0]):
        past = r[max(0, i - COV_DAYS) : i]
        past = past[~np.isnan(past).any(axis=1)]
        if past.shape[0] < COV_MIN_DAYS or np.isnan(w[i]).any():
            continue
        vol = float(np.sqrt(w[i] @ np.cov(past, rowvar=False) @ w[i] * 365))
        scale[i] = min(1.0, TARGET_VOL / vol) if vol > 0 else 1.0
    return scale


def oi_panel() -> pl.DataFrame:
    """
    Trial 2 panel over every available day (not yet cut to the evaluation period).
    """
    frames = []
    for s in run.SYMBOLS:
        targets = run.targets(run.daily_closes(s), len(run.SYMBOLS))
        metrics = pl.read_parquet(unsealed_metrics(s), columns=["time", "sum_open_interest"])
        frames.append(
            with_oi_weights(targets, oi_by_day(metrics)).with_columns(pl.lit(s).alias("symbol"))
        )
    return pl.concat(frames).sort("day", "symbol")


def capped(panel: pl.DataFrame) -> pl.DataFrame:
    """
    Apply the volatility cap to a full panel's weights.
    """
    cols = list(run.SYMBOLS)
    w = panel.pivot(on="symbol", index="day", values="w").sort("day")
    r = panel.pivot(on="symbol", index="day", values="next_ret").sort("day")
    scale = pl.DataFrame(
        {
            "day": w["day"],
            "vol_scale": vol_cap_scale(
                w.select(cols).to_numpy().astype(np.float64),
                r.select(cols).to_numpy().astype(np.float64),
            ),
        }
    )
    return panel.join(scale, on="day", how="left").with_columns(
        (pl.col("w") * pl.col("vol_scale")).alias("w"),
    )


def evaluation_period(panel: pl.DataFrame) -> pl.DataFrame:
    panel = panel.filter((pl.col("day") >= run.START_DAY) & (pl.col("day") < run.END_DAY - 1))
    if panel.null_count().select(["w", "next_ret"]).sum_horizontal().item():
        raise RuntimeError("missing weights or returns inside the evaluation period")
    return panel.sort("day", "symbol")


def main() -> None:
    full = oi_panel()
    print(f"=== Trial 2: {TRIAL_OI}")
    run.evaluate(evaluation_period(full), TRIAL_OI, OUT_DIR_OI)
    print(f"\n=== Trial 3: {TRIAL_CAP}")
    cap = evaluation_period(capped(full))
    run.evaluate(cap, TRIAL_CAP, OUT_DIR_CAP)
    binding = cap.group_by("day").agg(pl.col("vol_scale").first())["vol_scale"]
    print(f"cap binds on {(binding < 1).mean():.1%} of days, median scale {binding.median():.3f}")


if __name__ == "__main__":
    main()
