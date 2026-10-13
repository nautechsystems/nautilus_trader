"""
Build open-interest bars from Binance USD-M 5-minute metrics and 1-minute klines.

Rules:
- Timing: a metrics row stamped t describes the interval [t, t + 5 min) (its taker ratio matches
  the klines of that interval) and its open interest is the level at the end of it, so the row
  is known only at t + 5 min. Each 5-minute step is keyed by that end time.
- Position flow of a step = |change in open interest (contracts)| * close price at the step's
  end: the notional of positions opened or closed, without the revaluation of open positions.
- Threshold for UTC day D = mean daily position flow over the 30 most recent active days before
  D, divided by the target bars per day (the dollar-bar rule, with steps assigned to the day
  their interval starts in). Only past data is used; the first 30 active days are warm-up.
- A bar closes at the end of the first step where the running position flow reaches the
  threshold. Any excess in that step is discarded, so one step never produces several bars.
- Bars never span a data gap (a missing metrics row or a step without klines): a partial bar
  before a gap is dropped, as is the trailing partial bar at the end of the data.
- Price, volume and taker columns aggregate the 1-minute klines of the bar's steps. Open
  interest columns: level before the bar (`oi_open`), high / low / last level at step ends,
  net change in contracts, and net and gross position flow.
- Months from SEAL_MONTH on are never read.

A bar is known only at `close_time` (the last millisecond of its final step).

Output: <out-dir>/opd<N>/<SYMBOL>.parquet per target, plus <out-dir>/opd<N>/summary.csv.

Usage (from the repository root):
    python/.venv/Scripts/python research/oi_bars.py --bars-per-day 25

"""

from __future__ import annotations

import argparse
import re
from pathlib import Path

import numba
import numpy as np
import polars as pl

from dollar_bars import daily_thresholds
from dollar_bars import recover_decimals


STEP_MS = 300_000
DAY_MS = 86_400_000
SEAL_MONTH = (2026, 7)
MONTH_RE = re.compile(r"-(\d{4})-(\d{2})\.parquet$")
SUM_COLS = ("volume", "quote_volume", "count", "taker_buy_volume", "taker_buy_quote_volume")


def unsealed(paths: list[Path]) -> list[Path]:
    out = []
    for p in sorted(paths):
        m = MONTH_RE.search(p.name)
        if m is None:
            raise ValueError(f"Unexpected file name: {p}")
        if (int(m[1]), int(m[2])) < SEAL_MONTH:
            out.append(p)
    return out


def load_steps(symbol: str, klines_dir: Path, metrics_dir: Path) -> pl.DataFrame:
    """
    5-minute steps keyed by `end_time` (when the step is known), with klines aggregates and the
    open interest at the end of the step. Steps missing either source are absent.
    """
    k = (
        pl.read_parquet(unsealed(list((klines_dir / symbol).glob("*.parquet"))))
        .sort("open_time")
        .unique("open_time", keep="first", maintain_order=True)
    )
    k = k.with_columns(
        *[pl.Series(c, recover_decimals(k[c].to_numpy())) for c in ("open", "high", "low", "close")],
        *[pl.col(c).cast(pl.Float64) for c in SUM_COLS if c != "count"],
    )
    steps = (
        k.group_by((pl.col("open_time") // STEP_MS * STEP_MS + STEP_MS).alias("end_time"))
        .agg(
            pl.col("open").first(),
            pl.col("high").max(),
            pl.col("low").min(),
            pl.col("close").last(),
            *[pl.col(c).sum() for c in SUM_COLS],
            pl.len().alias("n_minutes"),
        )
    )
    oi = (
        pl.read_parquet(
            unsealed(list((metrics_dir / symbol).glob("*.parquet"))),
            columns=["time", "sum_open_interest"],
        )
        .select((pl.col("time") + STEP_MS).alias("end_time"), pl.col("sum_open_interest").alias("oi"))
        .drop_nulls()
        .unique("end_time", keep="first")
    )
    return (
        steps.join(oi, on="end_time", how="inner")
        .sort("end_time")
        .with_columns(
            # Change over the step; null after a gap so a bar never starts with a stale level
            pl.when(pl.col("end_time").diff() == STEP_MS)
            .then(pl.col("oi").diff())
            .otherwise(None)
            .alias("doi"),
        )
        .with_columns(
            (pl.col("doi") * pl.col("close")).alias("flow_net"),
            (pl.col("doi").abs() * pl.col("close")).alias("flow"),
            pl.col("oi").shift(1).alias("oi_prev"),
        )
    )


@numba.njit(cache=True)
def _segment(end_time, flow, threshold):
    n = end_time.shape[0]
    starts = np.empty(n, dtype=np.int64)
    ends = np.empty(n, dtype=np.int64)
    k = 0
    start = -1
    cum = 0.0
    for i in range(n):
        thr = threshold[i]
        gap = i == 0 or end_time[i] - end_time[i - 1] != STEP_MS or np.isnan(flow[i])
        if np.isnan(thr) or gap:
            # Warm-up or a data gap: drop the bar in progress; a step after a gap has no change
            start = -1
            cum = 0.0
            continue
        if start < 0:
            start = i
        cum += flow[i]
        if cum >= thr:
            starts[k] = start
            ends[k] = i
            k += 1
            start = -1
            cum = 0.0
    return starts[:k], ends[:k]


def build_bars(steps: pl.DataFrame, bars_per_day: int) -> pl.DataFrame:
    end_time = steps["end_time"].to_numpy()
    flow = steps["flow"].fill_null(np.nan).to_numpy()
    start_time = end_time - STEP_MS
    threshold = daily_thresholds(start_time, np.nan_to_num(flow), bars_per_day)
    starts, ends = _segment(end_time, flow, threshold)
    if starts.size == 0:
        return pl.DataFrame()

    bar_id = np.full(end_time.shape[0], -1, dtype=np.int64)
    for j in range(starts.size):
        bar_id[starts[j] : ends[j] + 1] = j

    return (
        steps.with_columns(pl.Series("bar", bar_id), pl.Series("threshold", threshold))
        .filter(pl.col("bar") >= 0)
        .group_by("bar", maintain_order=True)
        .agg(
            (pl.col("end_time").first() - STEP_MS).alias("open_time"),
            (pl.col("end_time").last() - 1).alias("close_time"),
            pl.col("open").first(),
            pl.col("high").max(),
            pl.col("low").min(),
            pl.col("close").last(),
            *[pl.col(c).sum() for c in SUM_COLS],
            pl.col("n_minutes").sum(),
            pl.len().alias("n_steps"),
            pl.col("oi_prev").first().alias("oi_open"),
            pl.max_horizontal(pl.col("oi").max(), pl.col("oi_prev").first()).alias("oi_high"),
            pl.min_horizontal(pl.col("oi").min(), pl.col("oi_prev").first()).alias("oi_low"),
            pl.col("oi").last().alias("oi_close"),
            pl.col("doi").sum().alias("oi_change"),
            pl.col("flow_net").sum(),
            pl.col("flow").sum(),
            pl.col("threshold").last(),
        )
        .drop("bar")
    )


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--klines-dir", type=Path, default=Path("binance_data/klines_1m"))
    parser.add_argument("--metrics-dir", type=Path, default=Path("binance_data/futures_metrics"))
    parser.add_argument("--out-dir", type=Path, default=Path("binance_data/oi_bars"))
    parser.add_argument("--bars-per-day", type=int, nargs="+", default=[25])
    args = parser.parse_args()

    summaries: dict[int, list[dict]] = {bpd: [] for bpd in args.bars_per_day}
    for bpd in args.bars_per_day:
        (args.out_dir / f"opd{bpd}").mkdir(parents=True, exist_ok=True)

    symbols = sorted(p.name for p in args.metrics_dir.iterdir() if p.is_dir())
    for i, symbol in enumerate(symbols, 1):
        steps = load_steps(symbol, args.klines_dir, args.metrics_dir)
        for bpd in args.bars_per_day:
            bars = build_bars(steps, bpd)
            # Symbols too short for the warm-up get no file
            if bars.height:
                bars.write_parquet(args.out_dir / f"opd{bpd}" / f"{symbol}.parquet")
                days = (bars["close_time"].max() - bars["open_time"].min() + 1) / DAY_MS
                summaries[bpd].append(
                    {
                        "symbol": symbol,
                        "bars": bars.height,
                        "first_bar": bars["open_time"].min(),
                        "last_bar": bars["close_time"].max(),
                        "bars_per_day": bars.height / days,
                        "median_minutes": bars["n_minutes"].median(),
                        "one_step_share": (bars["n_steps"] == 1).mean(),
                        "median_overshoot": (bars["flow"] / bars["threshold"]).median(),
                    },
                )
        print(f"[{i}/{len(symbols)}] {symbol}", flush=True)

    for bpd, rows in summaries.items():
        summary = pl.DataFrame(rows).with_columns(
            pl.from_epoch("first_bar", time_unit="ms"),
            pl.from_epoch("last_bar", time_unit="ms"),
        )
        summary.write_csv(args.out_dir / f"opd{bpd}" / "summary.csv")
        print(
            f"opd{bpd}: {summary['bars'].sum()} bars, realized bars/day median "
            f"{summary['bars_per_day'].median():.2f}, one-step bars "
            f"{summary['one_step_share'].mean():.2%}, median overshoot "
            f"{summary['median_overshoot'].median():.2f}",
        )


if __name__ == "__main__":
    main()
