"""
Parameter robustness of the trend strategy: a diagnostic, not a trial. Nothing is registered and
the strategy's rules stay as they are whatever the neighbours show.

Neighbours of the rules, all evaluated on the common period COMMON_START_DAY .. 2026-06-30
(the longest lookback needs 112 days of history):

- every lookback scaled by 0.5, 0.75, 1, 1.25, 1.5 and 2,
- volatility windows of 20, 30 and 60 days,
- rebalancing at 00:00, 08:00 and 16:00 UTC,
- each lookback on its own.

Run from `research/`: `../python/.venv/Scripts/python -m trend.robustness`.
"""

from __future__ import annotations

import numpy as np
import polars as pl

from trend import common
from trend import run


COMMON_START_DAY = 19_113  # 2022-05-01
SCALES = (0.5, 0.75, 1.0, 1.25, 1.5, 2.0)
VOL_WINDOWS = (20, 30, 60)
HOURS = (0, 8, 16)


def evaluate(closes: dict[str, pl.DataFrame], lookbacks: tuple[int, ...], vol_days: int) -> dict:
    n = len(run.SYMBOLS)
    panel = (
        pl.concat(
            [
                run.targets(closes[s], n, lookbacks, vol_days).with_columns(
                    pl.lit(s).alias("symbol")
                )
                for s in run.SYMBOLS
            ]
        )
        .filter((pl.col("day") >= COMMON_START_DAY) & (pl.col("day") < run.END_DAY - 1))
        .sort("day", "symbol")
    )
    if panel.null_count().select(["w", "next_ret"]).sum_horizontal().item():
        raise RuntimeError(f"missing values for lookbacks {lookbacks}, vol {vol_days}")
    cols = list(run.SYMBOLS)
    w = panel.pivot(on="symbol", index="day", values="w").sort("day").select(cols).to_numpy()
    r = panel.pivot(on="symbol", index="day", values="next_ret").sort("day").select(cols)
    ret, turnover, _ = run.backtest(w, r.to_numpy())
    days = panel["day"].unique().sort().to_numpy() + 1
    equity = np.cumprod(1 + ret)
    dd, _ = common.drawdown_runs(ret)
    years = days * common.DAY_MS
    year = pl.from_epoch(pl.Series(years), time_unit="ms").dt.year().to_numpy()
    by_year = {
        f"ret_{y}": float(np.prod(1 + ret[year == y]) - 1) for y in (2022, 2023, 2024, 2025, 2026)
    }
    return {
        "sharpe": common.sharpe(ret) * np.sqrt(365),
        "cagr": float(equity[-1] ** (365 / ret.size) - 1),
        "max_dd": float(dd.max()),
        "sharpe_wo_top10": common.concentration(ret)["sharpe_annual_without_top10"],
        "turnover_yr": float(turnover.mean() * 365),
    } | by_year


def main() -> None:
    rows = []
    for hour in HOURS:
        closes = {s: run.daily_closes(s, hour) for s in run.SYMBOLS}
        if hour == 0:
            for f in SCALES:
                lb = tuple(max(1, round(n * f)) for n in run.LOOKBACKS)
                rows.append({"test": "lookback scale", "setting": f"x{f} {lb}"})
                rows[-1] |= evaluate(closes, lb, run.VOL_DAYS)
            for v in VOL_WINDOWS:
                rows.append({"test": "vol window", "setting": f"{v} days"})
                rows[-1] |= evaluate(closes, run.LOOKBACKS, v)
            for n in run.LOOKBACKS:
                rows.append({"test": "single lookback", "setting": f"{n} days only"})
                rows[-1] |= evaluate(closes, (n,), run.VOL_DAYS)
        rows.append({"test": "rebalance hour", "setting": f"{hour:02d}:00 UTC"})
        rows[-1] |= evaluate(closes, run.LOOKBACKS, run.VOL_DAYS)
        print(f"hour {hour} done", flush=True)
    out = pl.DataFrame(rows)
    out.write_csv(common.OUT_DIR / "robustness.csv")
    with pl.Config(tbl_rows=50, float_precision=3, tbl_width_chars=220, fmt_str_lengths=40):
        print(out)


if __name__ == "__main__":
    main()
