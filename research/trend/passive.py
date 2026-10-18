"""
Passive limit-order execution of trial 3 on 1-minute bars: a diagnostic, not a trial. Nothing is
registered and the signals, weights and volatility cap stay exactly as in trial 3.

Each rebalance is decided at the 00:00 UTC close (price P0) and executed over a window after it:

- taker at close (baseline): the whole trade at P0, taker fee.
- taker TWAP: SLICES equal market orders at the last price at the start of each slice interval,
  taker fee.
- passive TWAP: every interval releases 1/SLICES of the trade; all released but unfilled
  quantity rests as one limit order at the last price at the interval start (re-pegged each
  interval). It fills only if the market trades through the limit within the interval (a
  1-minute low strictly below a buy limit, a high strictly above a sell limit), at the limit,
  maker fee. Whatever is unfilled at the window end is bought or sold at the last price, taker
  fee.

Costs are measured as implementation shortfall against P0: the book earns trial 3's
close-to-close return and each traded weight pays side * (fill / P0 - 1) plus its fee, which is
exact for orders that complete. The last rebalance's window (2026-06-30 00:00 onward) stays
before the sealed month.

Fill caveats: whole orders fill at once (our orders are tiny against 1-minute volume); queue
position is approximated by the trade-through rule; no latency, spread or partial fills.

Run from `research/`: `../python/.venv/Scripts/python -m trend.passive`.
"""

from __future__ import annotations

import numpy as np
import polars as pl

from trend import common
from trend import run
from trend import vol_target


MINUTE_MS = 60_000
TAKER_FEE = run.FEE
MAKER_FEE = 0.0002
# (name, kind, window minutes, slices)
VARIANTS = (
    ("taker at 00:00 close (baseline)", "close", 0, 1),
    ("taker TWAP 1h / 6", "taker", 60, 6),
    ("passive TWAP 1h / 6", "passive", 60, 6),
    ("taker TWAP 4h / 24", "taker", 240, 24),
    ("passive TWAP 4h / 24", "passive", 240, 24),
)


def daily_p0(symbol: str, days: np.ndarray) -> np.ndarray:
    """
    Decision price at the close of each day, carried over missing days as in run.targets.
    """
    c = run.daily_closes(symbol)
    grid = pl.DataFrame({"day": np.arange(c["day"].min(), days.max() + 1)})
    full = (
        grid.join(c, on="day", how="left").sort("day").with_columns(pl.col("close").forward_fill())
    )
    return full.filter(pl.col("day").is_in(days.tolist())).sort("day")["close"].to_numpy()


def window_bars(symbol: str, days: np.ndarray, window: int) -> dict[str, np.ndarray]:
    """
    Low, high and close of the 1-minute bars in the `window` minutes after each day's close, as
    (days, window) arrays with NaN for missing bars.
    """
    m = (
        common.load_minutes(symbol)
        .with_columns(
            (pl.col("open_time") // common.DAY_MS - 1).alias("day"),
            (pl.col("open_time") % common.DAY_MS // MINUTE_MS).alias("idx"),
        )
        .filter(pl.col("idx") < window)
    )
    day, idx = m["day"].to_numpy(), m["idx"].to_numpy()
    pos = np.clip(np.searchsorted(days, day), 0, days.size - 1)
    ok = days[pos] == day
    out = {}
    for col in ("low", "high", "close"):
        a = np.full((days.size, window), np.nan)
        a[pos[ok], idx[ok]] = m[col].to_numpy()[ok]
        out[col] = a
    return out


def last_prices(p0: np.ndarray, close: np.ndarray) -> np.ndarray:
    """
    Last traded price at the end of each window minute, starting from P0 and carried over gaps.
    """
    last = np.empty_like(close)
    prev = p0.copy()
    for j in range(close.shape[1]):
        prev = np.where(np.isnan(close[:, j]), prev, close[:, j])
        last[:, j] = prev
    return last


def unit_costs(
    kind: str,
    p0: np.ndarray,
    bars: dict[str, np.ndarray] | None,
    slices: int,
) -> dict[int, tuple[np.ndarray, np.ndarray]]:
    """
    Per side (+1 buy, -1 sell): cost per unit of traded weight and the passively filled share.
    """
    n = p0.size
    if kind == "close":
        return {s: (np.full(n, TAKER_FEE), np.zeros(n)) for s in (1, -1)}
    window = bars["close"].shape[1]
    step = window // slices
    last = last_prices(p0, bars["close"])
    # Last price at the start of each interval (P0 for the first)
    px = np.column_stack([p0] + [last[:, k * step - 1] for k in range(1, slices)])
    if kind == "taker":
        return {
            s: (s * (px / p0[:, None] - 1).mean(axis=1) + TAKER_FEE, np.zeros(n)) for s in (1, -1)
        }
    out = {}
    for s in (1, -1):
        filled = np.zeros(n)
        cost = np.zeros(n)
        for k in range(slices):
            seg = slice(k * step, (k + 1) * step)
            if s == 1:
                through = np.nan_to_num(bars["low"][:, seg], nan=np.inf).min(axis=1) < px[:, k]
            else:
                through = np.nan_to_num(bars["high"][:, seg], nan=-np.inf).max(axis=1) > px[:, k]
            open_qty = (k + 1) / slices - filled
            hit = through & (open_qty > 0)
            cost += np.where(hit, open_qty * (s * (px[:, k] / p0 - 1) + MAKER_FEE), 0.0)
            filled += np.where(hit, open_qty, 0.0)
        rest = 1 - filled
        cost += rest * (s * (last[:, -1] / p0 - 1) + TAKER_FEE)
        out[s] = (cost, filled)
    return out


def book(
    w: np.ndarray,
    r: np.ndarray,
    cost_buy: np.ndarray,
    cost_sell: np.ndarray,
    share_buy: np.ndarray,
    share_sell: np.ndarray,
) -> tuple[np.ndarray, float, float]:
    """
    Daily returns after execution costs, as run.backtest with per-trade costs; also the
    passively filled share of traded weight and the mean cost per unit traded.
    """
    held = np.zeros(w.shape[1])
    ret = np.zeros(w.shape[0])
    traded = passive = paid = 0.0
    for i in range(w.shape[0]):
        trade = w[i] - held
        buy = trade > 0
        size = np.abs(trade)
        cost = float((size * np.where(buy, cost_buy[i], cost_sell[i])).sum())
        gross = float(w[i] @ r[i])
        ret[i] = gross - cost
        traded += size.sum()
        passive += float((size * np.where(buy, share_buy[i], share_sell[i])).sum())
        paid += cost
        held = w[i] * (1 + r[i]) / (1 + gross)
    return ret, passive / traded, paid / traded


def main() -> None:
    panel = vol_target.evaluation_period(vol_target.capped(vol_target.oi_panel()))
    cols = list(run.SYMBOLS)
    w = panel.pivot(on="symbol", index="day", values="w").sort("day")
    r = panel.pivot(on="symbol", index="day", values="next_ret").sort("day")
    days = w["day"].to_numpy()
    wm, rm = w.select(cols).to_numpy(), r.select(cols).to_numpy()
    p0 = {s: daily_p0(s, days) for s in cols}
    bars_cache: dict[tuple[str, int], dict[str, np.ndarray]] = {}

    rows = []
    for name, kind, window, slices in VARIANTS:
        per = {}
        for s in cols:
            bars = None
            if kind != "close":
                if (s, window) not in bars_cache:
                    bars_cache[(s, window)] = window_bars(s, days, window)
                bars = bars_cache[(s, window)]
            per[s] = unit_costs(kind, p0[s], bars, slices)
        stack = {
            (side, j): np.column_stack([per[s][side][j] for s in cols])
            for side in (1, -1)
            for j in (0, 1)
        }
        ret, passive_share, unit_cost = book(
            wm, rm, stack[(1, 0)], stack[(-1, 0)], stack[(1, 1)], stack[(-1, 1)]
        )
        equity = np.cumprod(1 + ret)
        dd, _ = common.drawdown_runs(ret)
        years = ret.size / 365
        rows.append(
            {
                "execution": name,
                "sharpe": common.sharpe(ret) * np.sqrt(365),
                "cagr": float(equity[-1] ** (1 / years) - 1),
                "max_dd": float(dd.max()),
                "sharpe_wo_top10": common.concentration(ret)["sharpe_annual_without_top10"],
                "passive_fill_share": passive_share,
                "cost_bps_per_trade": unit_cost * 1e4,
            },
        )
        print(f"done {name}", flush=True)

    out = pl.DataFrame(rows)
    out.write_csv(vol_target.OUT_DIR_CAP / "passive.csv")
    with pl.Config(tbl_rows=20, float_precision=4, tbl_width_chars=220, fmt_str_lengths=40):
        print(out)


if __name__ == "__main__":
    main()
