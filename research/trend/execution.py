"""
Execution-price sensitivity of trial 3: a diagnostic, not a trial. Nothing is registered and the
signals, weights and volatility cap stay exactly as in trial 3 (computed at the 00:00 UTC close).

Only the fill price changes. The baseline fills at the 00:00 close itself; each variant fills
every rebalance at either
- a single later minute (the close of the 1-minute bar ending `minutes` after 00:00), or
- a TWAP: the mean of the closes of the 1-minute bars ending in the first `minutes` after 00:00,
and the book's daily return is earned between consecutive fills. Fees stay 5 bps per side.
The sample ends one day before trial 3's (see main).

Run from `research/`: `../python/.venv/Scripts/python -m trend.execution`.
"""

from __future__ import annotations

import numpy as np
import polars as pl

from trend import common
from trend import run
from trend import vol_target


MINUTE_MS = 60_000
# (name, kind, minutes): kind "point" fills at the close of minute `minutes`, "twap" averages
# the closes of minutes 1..`minutes` after 00:00
VARIANTS = (
    ("00:00 close (baseline)", "point", 0),
    ("point 00:05", "point", 5),
    ("point 00:30", "point", 30),
    ("point 01:00", "point", 60),
    ("TWAP 00:00-00:30", "twap", 30),
    ("TWAP 00:00-01:00", "twap", 60),
    ("TWAP 00:00-04:00", "twap", 240),
)


def fill_prices(symbol: str, kind: str, minutes: int) -> pl.DataFrame:
    """
    Fill price of the rebalance at the close of each `day` (the boundary (day + 1) * DAY_MS).
    """
    m = common.load_minutes(symbol).select("close_time", "close")
    # Minutes after the boundary whose bar closes within the window, keyed by the boundary's day
    after = m.with_columns(
        ((pl.col("close_time") + 1) // common.DAY_MS - 1).alias("day"),
        ((pl.col("close_time") + 1) % common.DAY_MS // MINUTE_MS).alias("minute"),
    )
    if minutes == 0:
        picked = run.daily_closes(symbol)
    elif kind == "point":
        picked = after.filter(pl.col("minute") == minutes).select("day", pl.col("close"))
    else:
        picked = (
            after.filter((pl.col("minute") >= 1) & (pl.col("minute") <= minutes))
            .group_by("day")
            .agg(pl.col("close").mean())
        )
    # Days without a fill (data gaps) carry the last one forward, as run.targets does for closes
    grid = pl.DataFrame({"day": np.arange(picked["day"].min(), picked["day"].max() + 1)})
    return (
        grid.join(picked.rename({"close": "fill"}), on="day", how="left")
        .sort("day")
        .with_columns(pl.col("fill").forward_fill())
    )


def stats(ret: np.ndarray) -> dict:
    equity = np.cumprod(1 + ret)
    dd, _ = common.drawdown_runs(ret)
    return {
        "sharpe": common.sharpe(ret) * np.sqrt(365),
        "cagr": float(equity[-1] ** (365 / ret.size) - 1),
        "max_dd": float(dd.max()),
        "sharpe_wo_top10": common.concentration(ret)["sharpe_annual_without_top10"],
    }


def main() -> None:
    panel = vol_target.evaluation_period(vol_target.capped(vol_target.oi_panel()))
    cols = list(run.SYMBOLS)
    # The last rebalance's delayed fills would fall in the sealed month: every variant, the
    # baseline included, is evaluated without that final day
    w = panel.pivot(on="symbol", index="day", values="w").sort("day")[:-1]
    days = w["day"]
    wm = w.select(cols).to_numpy()

    rows = []
    for name, kind, minutes in VARIANTS:
        rets = []
        for s in cols:
            f = fill_prices(s, kind, minutes)
            # Held from this rebalance's fill to the next one's
            nxt = f.with_columns((pl.col("fill").shift(-1) / pl.col("fill") - 1).alias("r"))
            nxt = nxt.filter(pl.col("day").is_in(days.implode()))
            if nxt.height != days.len() or nxt["r"].null_count():
                raise RuntimeError(f"missing fills for {s} ({name})")
            rets.append(nxt.sort("day")["r"].to_numpy())
        ret, _, _ = run.backtest(wm, np.column_stack(rets))
        rows.append({"execution": name} | stats(ret))
        print(f"done {name}", flush=True)

    out = pl.DataFrame(rows)
    out.write_csv(vol_target.OUT_DIR_CAP / "execution.csv")
    with pl.Config(tbl_rows=20, float_precision=4, tbl_width_chars=200, fmt_str_lengths=30):
        print(out)


if __name__ == "__main__":
    main()
