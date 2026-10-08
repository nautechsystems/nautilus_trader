"""
Build approximate dollar bars from the Binance USD-M 1-minute kline archive.

Rules:
- Threshold for UTC day D = mean daily quote volume over the 30 most recent active days
  before D, divided by the target bars per day. Active days have non-zero volume, so trading
  halts and days missing from the archive are skipped rather than dragging the threshold
  down. Only past data is used. The first 30 active days of each symbol are warm-up and
  produce no bars.
- Quote volume is accumulated minute by minute; a bar closes at the end of the first minute
  where the running total reaches the threshold. Any excess in that minute is discarded
  rather than carried, so one minute never produces several identical bars.
- Bars never span a data gap: a partial bar before a gap is dropped, as is the trailing
  partial bar at the end of the data.
- Prices are stored as float32 in the archive. For each symbol they are rounded to the
  fewest decimals that stay within one float32 ulp of every raw value, which recovers the
  original decimal prices. Volumes cannot be recovered and are float32 sums.

A bar is known only at `close_time` (the last millisecond of its final minute).

Output: <out-dir>/bpd<N>/<SYMBOL>.parquet per target, plus <out-dir>/bpd<N>/summary.csv.

Usage (from the repository root):
    python/.venv/Scripts/python research/dollar_bars.py --bars-per-day 12 25

"""

from __future__ import annotations

import argparse
from pathlib import Path

import numba
import numpy as np
import polars as pl


MINUTE_MS = 60_000
DAY_MS = 86_400_000
WINDOW_DAYS = 30
PRICE_COLS = ("open", "high", "low", "close")
SUM_COLS = ("volume", "quote_volume", "count", "taker_buy_volume", "taker_buy_quote_volume")


def recover_decimals(raw: np.ndarray, max_decimals: int = 10) -> np.ndarray:
    """
    Round float32 prices to the fewest decimals within one float32 ulp of every value.
    """
    x = raw.astype(np.float64)
    tol = np.spacing(np.abs(raw)).astype(np.float64)
    for d in range(max_decimals + 1):
        rounded = np.round(x, d)
        if np.all(np.abs(rounded - x) <= tol):
            return rounded
    raise ValueError("prices do not map to a decimal grid within float32 precision")


def daily_thresholds(
    open_time: np.ndarray,
    quote_volume: np.ndarray,
    bars_per_day: int,
) -> np.ndarray:
    """
    Return the per-minute threshold from the mean quote volume of the prior 30 active days.
    """
    day = open_time // DAY_MS
    # Halted days (zero volume) and days missing from the archive are skipped, so the window
    # always reflects 30 days of actual trading strictly before the current day
    active = (
        pl.DataFrame({"day": day, "qv": quote_volume.astype(np.float64)})
        .group_by("day")
        .agg(pl.col("qv").sum())
        .filter(pl.col("qv") > 0)
        .sort("day")
        .with_columns(
            (
                pl.col("qv").shift(1).rolling_mean(window_size=WINDOW_DAYS, min_samples=WINDOW_DAYS)
                / bars_per_day
            ).alias("threshold"),
        )
    )
    lookup = pl.DataFrame({"day": day}).join(
        active.select("day", "threshold"), on="day", how="left"
    )
    return lookup["threshold"].fill_null(np.nan).to_numpy()


@numba.njit(cache=True)
def _segment(open_time, quote_volume, threshold):
    n = open_time.shape[0]
    starts = np.empty(n, dtype=np.int64)
    ends = np.empty(n, dtype=np.int64)
    k = 0
    start = -1
    cum = 0.0
    for i in range(n):
        thr = threshold[i]
        if np.isnan(thr) or (i > 0 and open_time[i] - open_time[i - 1] != 60_000):
            # Warm-up or a data gap: drop the bar in progress
            start = -1
            cum = 0.0
            if np.isnan(thr):
                continue
        if start < 0:
            start = i
        cum += quote_volume[i]
        if cum >= thr:
            starts[k] = start
            ends[k] = i
            k += 1
            start = -1
            cum = 0.0
    return starts[:k], ends[:k]


def build_bars(df: pl.DataFrame, bars_per_day: int) -> pl.DataFrame:
    """
    Aggregate a symbol's sorted 1-minute klines into approximate dollar bars.
    """
    open_time = df["open_time"].to_numpy()
    qv = df["quote_volume"].to_numpy().astype(np.float64)
    threshold = daily_thresholds(open_time, qv, bars_per_day)
    starts, ends = _segment(open_time, qv, threshold)
    if starts.size == 0:
        return pl.DataFrame()

    bar_id = np.full(open_time.shape[0], -1, dtype=np.int64)
    for j in range(starts.size):
        bar_id[starts[j] : ends[j] + 1] = j

    return (
        df.with_columns(
            pl.Series("bar", bar_id),
            pl.Series("threshold", threshold),
        )
        .filter(pl.col("bar") >= 0)
        .group_by("bar", maintain_order=True)
        .agg(
            pl.col("open_time").first(),
            pl.col("close_time").last(),
            pl.col("open").first(),
            pl.col("high").max(),
            pl.col("low").min(),
            pl.col("close").last(),
            *[pl.col(c).sum() for c in SUM_COLS],
            pl.len().alias("n_minutes"),
            pl.col("threshold").last(),
        )
        .drop("bar")
    )


def load_symbol(sym_dir: Path) -> pl.DataFrame:
    df = (
        pl.read_parquet(sorted(sym_dir.glob("*.parquet")))
        .sort("open_time")
        .unique("open_time", keep="first", maintain_order=True)
    )
    return df.with_columns(
        *[pl.Series(c, recover_decimals(df[c].to_numpy())) for c in PRICE_COLS],
        *[pl.col(c).cast(pl.Float64) for c in SUM_COLS if c != "count"],
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--data-dir", type=Path, default=Path("binance_data/klines_1m"))
    parser.add_argument("--out-dir", type=Path, default=Path("binance_data/dollar_bars"))
    parser.add_argument("--bars-per-day", type=int, nargs="+", default=[12, 25])
    args = parser.parse_args()

    summaries: dict[int, list[dict]] = {bpd: [] for bpd in args.bars_per_day}
    for bpd in args.bars_per_day:
        (args.out_dir / f"bpd{bpd}").mkdir(parents=True, exist_ok=True)

    sym_dirs = sorted(p for p in args.data_dir.iterdir() if p.is_dir())
    for i, sym_dir in enumerate(sym_dirs, 1):
        df = load_symbol(sym_dir)
        for bpd in args.bars_per_day:
            bars = build_bars(df, bpd)
            bars.write_parquet(args.out_dir / f"bpd{bpd}" / f"{sym_dir.name}.parquet")
            if bars.height:
                days = (bars["close_time"].max() - bars["open_time"].min() + 1) / DAY_MS
                summaries[bpd].append(
                    {
                        "symbol": sym_dir.name,
                        "bars": bars.height,
                        "first_bar": bars["open_time"].min(),
                        "last_bar": bars["close_time"].max(),
                        "bars_per_day": bars.height / days,
                        "median_minutes": bars["n_minutes"].median(),
                        "one_minute_share": (bars["n_minutes"] == 1).mean(),
                        "median_overshoot": (bars["quote_volume"] / bars["threshold"]).median(),
                    },
                )
        print(f"[{i}/{len(sym_dirs)}] {sym_dir.name}")

    for bpd, rows in summaries.items():
        summary = pl.DataFrame(rows).with_columns(
            pl.from_epoch("first_bar", time_unit="ms"),
            pl.from_epoch("last_bar", time_unit="ms"),
        )
        summary.write_csv(args.out_dir / f"bpd{bpd}" / "summary.csv")
        print(
            f"bpd{bpd}: {summary['bars'].sum()} bars, realized bars/day median "
            f"{summary['bars_per_day'].median():.2f}, one-minute bars "
            f"{summary['one_minute_share'].mean():.2%}",
        )


if __name__ == "__main__":
    main()
