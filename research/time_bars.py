"""
Build fixed-interval time bars from the Binance USD-M 1-minute kline archive.

Rules:
- Bars of N minutes are aligned to the UTC epoch, so N must divide a day (1440 minutes): a
  10-minute bar opens at :00, :10, ..., :50 and a 20-minute bar at :00, :20, :40.
- Only complete bars are kept: a bar missing any of its N minutes (a data gap, the start of a
  listing) is dropped, so every bar covers exactly N consecutive minutes. Minutes with zero
  trades are present in the archive and count as complete.
- Prices are recovered to their decimal grid as in `dollar_bars.py`. Volumes are float32 sums.
- Months from SEAL_MONTH on are never read.

A bar is known only at `close_time` (open_time + N minutes - 1 ms, the Binance convention).

Output: <out-dir>/m<N>/<SYMBOL>.parquet per interval, plus <out-dir>/m<N>/summary.csv.

Usage (from the repository root):
    python/.venv/Scripts/python research/time_bars.py --minutes 10 20

"""

from __future__ import annotations

import argparse
from pathlib import Path

import polars as pl

from dollar_bars import recover_decimals
from oi_bars import unsealed


MINUTE_MS = 60_000
DAY_MINUTES = 1440
PRICE_COLS = ("open", "high", "low", "close")
SUM_COLS = ("volume", "quote_volume", "count", "taker_buy_volume", "taker_buy_quote_volume")


def load_symbol(sym_dir: Path) -> pl.DataFrame:
    df = (
        pl.read_parquet(unsealed(list(sym_dir.glob("*.parquet"))))
        .sort("open_time")
        .unique("open_time", keep="first", maintain_order=True)
    )
    return df.with_columns(
        *[pl.Series(c, recover_decimals(df[c].to_numpy())) for c in PRICE_COLS],
        *[pl.col(c).cast(pl.Float64) for c in SUM_COLS if c != "count"],
    )


def build_bars(df: pl.DataFrame, minutes: int) -> tuple[pl.DataFrame, int]:
    """
    Aggregate a symbol's sorted 1-minute klines into complete N-minute bars.

    Returns the bars and the number of incomplete bars dropped.

    """
    bar_ms = minutes * MINUTE_MS
    bars = (
        df.with_columns((pl.col("open_time") // bar_ms * bar_ms).alias("bar_open"))
        .group_by("bar_open", maintain_order=True)
        .agg(
            pl.col("open").first(),
            pl.col("high").max(),
            pl.col("low").min(),
            pl.col("close").last(),
            *[pl.col(c).sum() for c in SUM_COLS],
            pl.len().alias("n_minutes"),
        )
    )
    complete = bars.filter(pl.col("n_minutes") == minutes)
    out = complete.select(
        pl.col("bar_open").alias("open_time"),
        (pl.col("bar_open") + bar_ms - 1).alias("close_time"),
        *PRICE_COLS,
        *SUM_COLS,
    )
    return out, bars.height - complete.height


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--data-dir", type=Path, default=Path("data/binance/klines_1m"))
    parser.add_argument("--out-dir", type=Path, default=Path("data/derived/time_bars"))
    parser.add_argument("--minutes", type=int, nargs="+", default=[10, 20])
    args = parser.parse_args()

    for m in args.minutes:
        if m < 2 or DAY_MINUTES % m:
            raise SystemExit(f"--minutes {m} must be at least 2 and divide {DAY_MINUTES}")
        (args.out_dir / f"m{m}").mkdir(parents=True, exist_ok=True)

    summaries: dict[int, list[dict]] = {m: [] for m in args.minutes}
    sym_dirs = sorted(p for p in args.data_dir.iterdir() if p.is_dir())
    for i, sym_dir in enumerate(sym_dirs, 1):
        df = load_symbol(sym_dir)
        for m in args.minutes:
            bars, dropped = build_bars(df, m)
            bars.write_parquet(args.out_dir / f"m{m}" / f"{sym_dir.name}.parquet")
            if bars.height:
                summaries[m].append(
                    {
                        "symbol": sym_dir.name,
                        "bars": bars.height,
                        "dropped_incomplete": dropped,
                        "first_bar": bars["open_time"].min(),
                        "last_bar": bars["close_time"].max(),
                        "zero_trade_bars": int((bars["count"] == 0).sum()),
                    },
                )
        print(f"[{i}/{len(sym_dirs)}] {sym_dir.name}")

    for m, rows in summaries.items():
        summary = pl.DataFrame(rows).with_columns(
            pl.from_epoch("first_bar", time_unit="ms"),
            pl.from_epoch("last_bar", time_unit="ms"),
        )
        summary.write_csv(args.out_dir / f"m{m}" / "summary.csv")
        print(
            f"m{m}: {summary['bars'].sum()} bars over {summary.height} symbols, "
            f"dropped incomplete {summary['dropped_incomplete'].sum()}",
        )


if __name__ == "__main__":
    main()
