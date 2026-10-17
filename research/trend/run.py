"""
Daily time-series trend following on 13 established coins (a rule-based strategy with nothing
fitted).

Design fixed before the first run:

- Universe: the 13 USDT perpetuals with 1-minute klines from 2022-01, each traded on its own.
- Daily closes at 00:00 UTC from 1-minute klines; data up to 2026-06 (2026-07 onward sealed).
- Signal at a close: the mean over LOOKBACKS of the sign of the log return over that many days,
  in [-1, 1]. Long and short.
- Weight for the next day: signal * TARGET_VOL / sigma_annual / N, capped at 1 / N in absolute
  value, where sigma_annual is the standard deviation of daily log returns over the previous
  VOL_DAYS days times sqrt(365). The book is rebalanced to these weights at every close.
- Costs: fee FEE per side on the traded weight (from the drifted weights to the new targets),
  no slippage, no funding.
- Evaluation: 2022-03-01 .. 2026-06-30; go / no-go on every criterion in CRITERIA. Factor
  regressions use BTC and the equal-weight return of the 13 coins.

Run from `research/`: `../python/.venv/Scripts/python -m trend.run`.
"""

from __future__ import annotations

from pathlib import Path

import numpy as np
import polars as pl

from trend import common


TRIAL = "daily_tsmom:13_coins:lb7_14_28_56:vt40:long_short"
SYMBOLS = (
    "BTCUSDT", "ETHUSDT", "SOLUSDT", "BNBUSDT", "XRPUSDT", "DOGEUSDT", "ADAUSDT",
    "AVAXUSDT", "NEARUSDT", "1000SHIBUSDT", "GALAUSDT", "MANAUSDT", "SANDUSDT",
)  # fmt: skip
LOOKBACKS = (7, 14, 28, 56)
VOL_DAYS = 30
TARGET_VOL = 0.40
FEE = common.FEE
DAY_MS = common.DAY_MS
START_DAY = 19_052  # 2022-03-01
END_DAY = common.SEAL_START_MS // DAY_MS  # 2026-07-01, exclusive

CRITERIA = {
    "sharpe_annual": (">=", 1.0),
    "cagr": (">=", 0.20),
    "positive_periods": (">=", 3),
    "max_drawdown": ("<=", 0.30),
    "sharpe_without_top10_days": (">", 0.5),
    "dsr": (">=", 0.5),
}


def daily_closes(symbol: str, hour: int = 0) -> pl.DataFrame:
    """
    Close of each day at `hour` UTC (the last 1-minute close before it), keyed by `day`; with
    the default the day is the UTC day.
    """
    return (
        common.load_minutes(symbol)
        .group_by(((pl.col("close_time") - hour * 3_600_000) // DAY_MS).alias("day"))
        .agg(pl.col("close").sort_by("close_time").last())
        .sort("day")
    )


def targets(
    closes: pl.DataFrame,
    n_symbols: int,
    lookbacks: tuple[int, ...] = LOOKBACKS,
    vol_days: int = VOL_DAYS,
) -> pl.DataFrame:
    """
    Per day: the next day's simple return and the weight set at that day's close (signal and
    volatility use closes up to and including the day).
    """
    days = pl.DataFrame({"day": np.arange(closes["day"].min(), closes["day"].max() + 1)})
    d = days.join(closes, on="day", how="left").with_columns(pl.col("close").forward_fill())
    lp = pl.col("close").log()
    signal = pl.mean_horizontal([(lp - lp.shift(n)).sign() for n in lookbacks])
    sigma = lp.diff().rolling_std(vol_days) * np.sqrt(365)
    cap = 1.0 / n_symbols
    return d.select(
        "day",
        (pl.col("close").shift(-1) / pl.col("close") - 1).alias("next_ret"),
        signal.alias("signal"),
        (signal * TARGET_VOL / sigma / n_symbols).clip(-cap, cap).alias("w"),
    )


def backtest(w: np.ndarray, r: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """
    Daily portfolio return net of fees, turnover and gross exposure. Row i holds the weights set
    at close i and the returns from close i to close i + 1; weights drift with returns before
    the next rebalance.
    """
    n_days = w.shape[0]
    ret = np.zeros(n_days)
    turnover = np.zeros(n_days)
    held = np.zeros(w.shape[1])
    for i in range(n_days):
        turnover[i] = np.abs(w[i] - held).sum()
        gross_ret = float(w[i] @ r[i])
        ret[i] = gross_ret - FEE * turnover[i]
        held = w[i] * (1 + r[i]) / (1 + gross_ret)
    return ret, turnover, np.abs(w).sum(axis=1)


def build_panel() -> pl.DataFrame:
    """
    Per day and symbol over the evaluation period: signal, target weight and next-day return.
    """
    frames = []
    for s in SYMBOLS:
        frames.append(
            targets(daily_closes(s), len(SYMBOLS)).with_columns(pl.lit(s).alias("symbol"))
        )
    panel = (
        pl.concat(frames)
        .filter((pl.col("day") >= START_DAY) & (pl.col("day") < END_DAY - 1))
        .sort("day", "symbol")
    )
    if panel.null_count().select(["w", "next_ret"]).sum_horizontal().item():
        raise RuntimeError("missing weights or returns inside the evaluation period")
    return panel


def market_returns(panel: pl.DataFrame) -> pl.DataFrame:
    """
    BTC and equal-weight returns of the universe, keyed by the day they are realized.
    """
    return (
        panel.group_by("day")
        .agg(
            pl.col("next_ret").filter(pl.col("symbol") == "BTCUSDT").first().alias("btc"),
            pl.col("next_ret").mean().alias("ew"),
        )
        .with_columns(pl.col("day") + 1)
    )


def evaluate(panel: pl.DataFrame, trial: str, out_dir: Path) -> bool:
    """
    Backtest a panel (day, symbol, signal, w, next_ret), register the trial, write the report
    to `out_dir` and return whether every criterion in CRITERIA passes.
    """
    out_dir.mkdir(parents=True, exist_ok=True)
    w = panel.pivot(on="symbol", index="day", values="w").sort("day")
    r = panel.pivot(on="symbol", index="day", values="next_ret").sort("day")
    cols = list(SYMBOLS)
    ret, turnover, gross = backtest(w.select(cols).to_numpy(), r.select(cols).to_numpy())

    # A row's return is realized over the following day
    daily = pl.DataFrame(
        {
            "day": w["day"].to_numpy() + 1,
            "ret": ret,
            "gross": gross,
            "turnover": turnover,
            "trades": (np.abs(np.diff(w.select(cols).to_numpy(), axis=0, prepend=0)) > 0).sum(1),
            "active_symbols": (w.select(cols).to_numpy() != 0).sum(1),
        }
    )
    common.register(trial, common.sharpe(ret))
    trial_sr = common.load_registry()["sharpe_daily"].to_numpy()
    years = common.yearly(daily)
    contrib = (
        panel.with_columns((pl.col("w") * pl.col("next_ret")).alias("pnl"))
        .group_by("symbol")
        .agg(pl.col("pnl").sum(), pl.col("w").abs().mean().alias("avg_abs_w"))
        .sort("pnl", descending=True)
    )
    metrics = (
        common.performance(daily, trial_sr)
        | common.factor_regression(daily, market_returns(panel))
        | common.concentration(ret)
        | {
            "long_share_days": float((panel["w"] > 0).sum() / (panel["w"] != 0).sum()),
            "avg_abs_signal": float(panel["signal"].abs().mean()),
        }
    )
    checks = {
        "sharpe_annual": metrics["sharpe_annual"],
        "cagr": metrics["cagr"],
        "positive_periods": int((years["return_compounded"] > 0).sum()),
        "max_drawdown": metrics["max_drawdown"],
        "sharpe_without_top10_days": metrics["sharpe_annual_without_top10"],
        "dsr": metrics["dsr"],
    }
    out = pl.DataFrame({"metric": list(metrics), "value": [float(v) for v in metrics.values()]})
    out.write_csv(out_dir / "report.csv")
    years.write_csv(out_dir / "yearly.csv")
    daily.write_parquet(out_dir / "daily.parquet")
    contrib.write_csv(out_dir / "contributions.csv")
    with pl.Config(tbl_rows=200, float_precision=4, fmt_str_lengths=40):
        print(out)
        print(years)
        print(contrib)
    ops = {"<=": np.less_equal, ">=": np.greater_equal, ">": np.greater}
    for name, (op, bound) in CRITERIA.items():
        ok = ops[op](checks[name], bound)
        print(f"{'PASS' if ok else 'FAIL'}  {name} = {checks[name]:.4f} ({op} {bound})")
    go = all(ops[op](checks[n], b) for n, (op, b) in CRITERIA.items())
    print("GO" if go else "NO-GO")
    return go


def main() -> None:
    evaluate(build_panel(), TRIAL, common.OUT_DIR)


if __name__ == "__main__":
    main()
