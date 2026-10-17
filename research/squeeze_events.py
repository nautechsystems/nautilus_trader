"""
Event study: do 10/20-minute volatility-squeeze breakouts carry directional edge after costs?

Definitions (fixed before the first run, not tuned):
- Bars: `time_bars.py` output (m10, m20), which stops before the sealed months.
- Seasonality: each bar's log return r is divided by sqrt(s). s = mean r^2 of the same
  time-of-day slot over its previous 30 occurrences of the same day type (weekday / weekend),
  divided by the mean r^2 of all bars over the previous 30 calendar days. Past data only.
- Squeeze at breakout bar t: deseasonalized RV of the 2 hours before t divided by that of the
  7 days before t; squeezed when the ratio is at or below its 10% quantile over the previous
  30 days. Control group: the same breakouts without a squeeze.
- Breakout: close of bar t above the highest high (long) or below the lowest low (short) of
  the 2 hours before t.
- Cooldown: per symbol and group, no event within 2 hours after the previous kept one.
- Entry at the open of bar t + 1; outcome = direction * log(close of the bar ending at the
  horizon / entry), clock horizons 20m, 1h, 2h, 4h, 8h, 24h. Costs 5 bps per side.
- Daily trend: mean sign of 7/14/28/56-day log returns of UTC daily closes, from days before
  the event day. Aligned = breakout direction equals the trend sign.
- Standard errors clustered by UTC day (breakouts across symbols are simultaneous).
- vol_expand = deseasonalized RV of the 2 hours after t over that of the 2 hours before t.

Decision rule (pre-set): go on only if squeeze + trend-aligned on the 13 core symbols, for m10
or m20 at 1h/2h/4h/8h (8 looks), has mean gross >= 20 bps, clustered t >= 2.7 (Bonferroni
over 8 looks), is positive in at least 4 of the 5 years, and beats the aligned control group.

Output: <out-dir>/events.parquet, <out-dir>/summary.csv, <out-dir>/yearly.csv.

Usage (from the repository root):
    python/.venv/Scripts/python research/squeeze_events.py

"""

from __future__ import annotations

import argparse
from pathlib import Path

import numpy as np
import polars as pl


MINUTE_MS = 60_000
DAY_MS = 86_400_000
DAY_MINUTES = 1440
CORE = (
    "BTCUSDT",
    "ETHUSDT",
    "SOLUSDT",
    "BNBUSDT",
    "XRPUSDT",
    "DOGEUSDT",
    "ADAUSDT",
    "AVAXUSDT",
    "NEARUSDT",
    "1000SHIBUSDT",
    "GALAUSDT",
    "MANAUSDT",
    "SANDUSDT",
)
HORIZONS_MIN = (20, 60, 120, 240, 480, 1440)
DECISION_HORIZONS_MIN = (60, 120, 240, 480)
COST_BPS = 10.0
SEASON_N = 30
LEVEL_DAYS = 30
SHORT_MIN = 120
LONG_DAYS = 7
QUANTILE = 0.10
QUANTILE_DAYS = 30
TREND_DAYS = (7, 14, 28, 56)


def load_grid(path: Path, minutes: int) -> pl.DataFrame:
    """
    Load time bars onto a complete clock grid, missing bars as null rows.
    """
    bar_ms = minutes * MINUTE_MS
    df = pl.read_parquet(path, columns=["open_time", "open", "high", "low", "close"])
    grid = pl.int_range(
        df["open_time"].min(),
        df["open_time"].max() + 1,
        bar_ms,
        eager=True,
    ).alias("open_time")
    return pl.DataFrame(grid).join(df, on="open_time", how="left").sort("open_time")


def past_mean(x: pl.Expr, window: int, min_samples: int) -> pl.Expr:
    # Mean of the non-null values in the `window` rows before the current row
    return x.shift(1).rolling_mean(window, min_samples=min_samples)


def daily_trend(df: pl.DataFrame) -> pl.DataFrame:
    """
    Return the trend score per UTC day from closes of the days before it.
    """
    lc = pl.col("close").log()
    return (
        df.group_by("day")
        .agg(pl.col("close").drop_nulls().last())
        .sort("day")
        .with_columns(
            pl.mean_horizontal(
                [(lc.shift(1) - lc.shift(1 + n)).sign() for n in TREND_DAYS],
            ).alias("trend"),
        )
        .with_columns(
            # mean_horizontal skips nulls; require every lookback
            pl.when(lc.shift(1 + max(TREND_DAYS)).is_not_null()).then(pl.col("trend")),
        )
        .select("day", "trend")
    )


def features(df: pl.DataFrame, minutes: int) -> pl.DataFrame:
    bar_ms = minutes * MINUTE_MS
    per_day = DAY_MINUTES // minutes
    n = SHORT_MIN // minutes
    long_n = LONG_DAYS * per_day

    df = df.with_columns(
        pl.col("close").log().diff().alias("r"),
        ((pl.col("open_time") % DAY_MS) // bar_ms).alias("slot"),
        (pl.col("open_time") // DAY_MS).alias("day"),
    ).with_columns(
        (pl.col("r") ** 2).alias("r2"),
        # Epoch day 0 (1970-01-01) is a Thursday; Monday = 0
        (((pl.col("day") + 3) % 7) >= 5).alias("weekend"),
    )

    level = (
        df.group_by("day")
        .agg(pl.col("r2").sum().alias("s"), pl.col("r2").count().alias("c"))
        .sort("day")
        .with_columns(
            (
                pl.col("s").shift(1).rolling_sum(LEVEL_DAYS)
                / pl.col("c").shift(1).rolling_sum(LEVEL_DAYS)
            ).alias("level"),
        )
        .select("day", "level")
    )

    df = (
        df.join(level, on="day", how="left")
        .join(daily_trend(df), on="day", how="left")
        .sort("open_time")
        .with_columns(
            (past_mean(pl.col("r2"), SEASON_N, 20).over("slot", "weekend") / pl.col("level")).alias(
                "season"
            ),
        )
        .with_columns((pl.col("r2") / pl.col("season")).alias("z2"))
        .with_columns(
            (past_mean(pl.col("z2"), n, n) / past_mean(pl.col("z2"), long_n, long_n * 9 // 10))
            .sqrt()
            .alias("ratio"),
            past_mean(pl.col("z2"), n, n).sqrt().alias("rv_before"),
            pl.col("z2").rolling_mean(n, min_samples=n).shift(-n).sqrt().alias("rv_after"),
            pl.col("high").shift(1).rolling_max(n, min_samples=n).alias("chan_hi"),
            pl.col("low").shift(1).rolling_min(n, min_samples=n).alias("chan_lo"),
        )
        .with_columns(
            pl.col("ratio")
            .shift(1)
            .rolling_quantile(
                QUANTILE,
                window_size=QUANTILE_DAYS * per_day,
                min_samples=10 * per_day,
            )
            .alias("ratio_q"),
            pl.when(pl.col("close") > pl.col("chan_hi"))
            .then(1)
            .when(pl.col("close") < pl.col("chan_lo"))
            .then(-1)
            .otherwise(0)
            .alias("dir"),
            pl.col("open").shift(-1).alias("entry"),
        )
    )
    return df.with_columns(
        (pl.col("ratio") <= pl.col("ratio_q")).alias("squeezed"),
        (pl.col("rv_after") / pl.col("rv_before")).alias("vol_expand"),
        *[
            (
                pl.col("dir")
                * (pl.col("close").shift(-(h // minutes)).log() - pl.col("entry").log())
                * 1e4
            ).alias(f"fwd_{h}m")
            for h in HORIZONS_MIN
        ],
    )


def cooldown(times: np.ndarray, gap_ms: int) -> np.ndarray:
    keep = np.zeros(times.shape[0], dtype=bool)
    last = None
    for i, t in enumerate(times):
        if last is None or t - last >= gap_ms:
            keep[i] = True
            last = t
    return keep


def symbol_events(path: Path, minutes: int) -> pl.DataFrame:
    df = features(load_grid(path, minutes), minutes)
    ev = df.filter(
        (pl.col("dir") != 0) & pl.col("squeezed").is_not_null() & pl.col("entry").is_not_null(),
    )
    parts = []
    for squeezed in (True, False):
        g = ev.filter(pl.col("squeezed") == squeezed)
        parts.append(
            g.filter(pl.Series(cooldown(g["open_time"].to_numpy(), SHORT_MIN * MINUTE_MS)))
        )
    return pl.concat(parts).select(
        pl.lit(path.stem).alias("symbol"),
        pl.lit(minutes).alias("minutes"),
        "open_time",
        "day",
        "dir",
        "squeezed",
        "trend",
        "vol_expand",
        *[f"fwd_{h}m" for h in HORIZONS_MIN],
    )


def clustered(x: np.ndarray, day: np.ndarray) -> tuple[float, float]:
    """
    Return the mean and its standard error clustered by day.
    """
    m = x.mean()
    sums = pl.DataFrame({"d": day, "e": x - m}).group_by("d").agg(pl.col("e").sum())["e"]
    g = sums.len()
    se = float(np.sqrt((sums**2).sum() * g / max(g - 1, 1))) / x.shape[0]
    return float(m), se


def summarize(events: pl.DataFrame) -> tuple[pl.DataFrame, pl.DataFrame]:
    events = events.with_columns(
        pl.when(pl.col("trend") * pl.col("dir") > 0)
        .then(pl.lit("aligned"))
        .when(pl.col("trend") * pl.col("dir") < 0)
        .then(pl.lit("against"))
        .otherwise(pl.lit("flat"))
        .alias("trend_fit"),
        pl.from_epoch("open_time", time_unit="ms").dt.year().alias("year"),
    )
    rows, yearly = [], []
    for universe in ("core", "all"):
        u = events.filter(pl.col("symbol").is_in(CORE)) if universe == "core" else events
        for (minutes, squeezed), g0 in u.group_by("minutes", "squeezed"):
            for fit in ("any", "aligned", "against"):
                g = g0 if fit == "any" else g0.filter(pl.col("trend_fit") == fit)
                for h in HORIZONS_MIN:
                    col = f"fwd_{h}m"
                    s = g.filter(pl.col(col).is_not_null())
                    if s.height < 30:
                        continue
                    m, se = clustered(s[col].to_numpy(), s["day"].to_numpy())
                    key = {
                        "universe": universe,
                        "minutes": minutes,
                        "group": "squeeze" if squeezed else "control",
                        "trend_fit": fit,
                        "horizon_min": h,
                    }
                    rows.append(
                        key
                        | {
                            "events": s.height,
                            "days": s["day"].n_unique(),
                            "gross_bps": m,
                            "net_bps": m - COST_BPS,
                            "t": m / se,
                            "hit": float((s[col] > 0).mean()),
                            "vol_expand_median": s["vol_expand"].median(),
                        },
                    )
                    for (year,), y in s.group_by("year"):
                        yearly.append(
                            key | {"year": year, "events": y.height, "gross_bps": y[col].mean()}
                        )
    summary = pl.DataFrame(rows).sort("universe", "minutes", "group", "trend_fit", "horizon_min")
    by_year = pl.DataFrame(yearly).sort(
        "universe", "minutes", "group", "trend_fit", "horizon_min", "year"
    )
    return summary, by_year


def decide(summary: pl.DataFrame, by_year: pl.DataFrame) -> None:
    base = (pl.col("universe") == "core") & (pl.col("trend_fit") == "aligned")
    cells = summary.filter(base & pl.col("horizon_min").is_in(DECISION_HORIZONS_MIN))
    sq = cells.filter(pl.col("group") == "squeeze")
    ctrl = cells.filter(pl.col("group") == "control").select(
        "minutes", "horizon_min", pl.col("gross_bps").alias("control_bps")
    )
    years = (
        by_year.filter(base & (pl.col("group") == "squeeze"))
        .group_by("minutes", "horizon_min")
        .agg((pl.col("gross_bps") > 0).sum().alias("years_positive"), pl.len().alias("years"))
    )
    table = (
        sq.join(ctrl, on=["minutes", "horizon_min"], how="left")
        .join(years, on=["minutes", "horizon_min"], how="left")
        .with_columns(
            (
                (pl.col("gross_bps") >= 20)
                & (pl.col("t") >= 2.7)
                & (pl.col("years_positive") >= 4)
                & (pl.col("gross_bps") > pl.col("control_bps"))
            ).alias("pass"),
        )
        .select(
            "minutes",
            "horizon_min",
            "events",
            "gross_bps",
            "t",
            "control_bps",
            "years_positive",
            "years",
            "pass",
        )
        .sort("minutes", "horizon_min")
    )
    print("\nDecision cells (core, squeeze, trend-aligned):")
    print(table)
    print("GO to step 2" if table["pass"].any() else "NO-GO: no cell meets the pre-set rule")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("--bars-dir", type=Path, default=Path("binance_data/time_bars"))
    parser.add_argument("--out-dir", type=Path, default=Path("binance_data/squeeze_events"))
    parser.add_argument("--minutes", type=int, nargs="+", default=[10, 20])
    parser.add_argument("--symbols", nargs="+", default=None, help="limit to these symbols")
    args = parser.parse_args()

    parts = []
    for minutes in args.minutes:
        paths = sorted((args.bars_dir / f"m{minutes}").glob("*.parquet"))
        if args.symbols:
            paths = [p for p in paths if p.stem in args.symbols]
        for i, path in enumerate(paths, 1):
            parts.append(symbol_events(path, minutes))
            print(f"m{minutes} [{i}/{len(paths)}] {path.stem}")
    events = pl.concat(parts)

    args.out_dir.mkdir(parents=True, exist_ok=True)
    events.write_parquet(args.out_dir / "events.parquet")
    summary, by_year = summarize(events)
    summary.write_csv(args.out_dir / "summary.csv")
    by_year.write_csv(args.out_dir / "yearly.csv")
    decide(summary, by_year)


if __name__ == "__main__":
    main()
