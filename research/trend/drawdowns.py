"""
Drawdown attribution of trial 3 (OI confirmation + volatility cap): a diagnostic, not a trial.
Nothing is registered and the strategy's rules stay as they are.

For the deepest drawdown episodes (peak to trough), report:

- depth, days to trough and to recovery,
- P&L by side (long / short) and the three worst symbols,
- shock share: the three worst days' share of the episode's summed daily returns (high = a
  few sharp moves, low = a slow bleed),
- signal flips per symbol (changes of position sign; many = whipsaw in a range),
- the market over the episode (BTC and equal-weight return, realized vol against the
  60 days before), average gross exposure, volatility-cap scale and fees.

Run from `research/`: `../python/.venv/Scripts/python -m trend.drawdowns`.
"""

from __future__ import annotations

from datetime import UTC
from datetime import datetime

import numpy as np
import polars as pl

from trend import common
from trend import run
from trend import vol_target


TOP_EPISODES = 8
SHOCK_DAYS = 3


def episodes(ret: np.ndarray) -> list[tuple[int, int, int | None]]:
    """
    Drawdown episodes as (peak, trough, recovery) row indices into `ret`, deepest first.

    Row i is the return over day i; the peak is the last row at the prior high (-1 when the
    episode starts on the first day), the recovery the first row back at or above it.

    """
    equity = np.cumprod(1 + ret)
    out = []
    peak, peak_eq = -1, 1.0
    trough, trough_eq = None, None
    for i, e in enumerate(equity):
        if e >= peak_eq:
            if trough is not None:
                out.append((peak, trough, i))
            peak, peak_eq, trough, trough_eq = i, e, None, None
        elif trough_eq is None or e < trough_eq:
            trough, trough_eq = i, e
    if trough is not None:
        out.append((peak, trough, None))

    def depth(ep: tuple[int, int, int | None]) -> float:
        p, t, _ = ep
        start = equity[p] if p >= 0 else 1.0
        return equity[t] / start - 1

    return sorted(out, key=depth)[:TOP_EPISODES]


def date(day: int) -> str:
    return datetime.fromtimestamp(day * common.DAY_MS / 1000, UTC).strftime("%Y-%m-%d")


def main() -> None:
    panel = vol_target.evaluation_period(vol_target.capped(vol_target.oi_panel()))
    cols = list(run.SYMBOLS)

    def wide(col: str) -> np.ndarray:
        return panel.pivot(on="symbol", index="day", values=col).sort("day").select(cols).to_numpy()

    w, r, sig = wide("w"), wide("next_ret"), wide("signal")
    days = panel["day"].unique().sort().to_numpy() + 1  # day each row's return is realized
    scale = panel.group_by("day").agg(pl.col("vol_scale").first()).sort("day")["vol_scale"]
    ret, turnover, gross = run.backtest(w, r)
    print(f"Sharpe {common.sharpe(ret) * np.sqrt(365):.4f} (trial 3 report: 1.2054)")

    pnl = w * r  # per-symbol return contribution of each day, before fees
    fees = run.FEE * turnover
    ew = r.mean(axis=1)
    btc = r[:, cols.index("BTCUSDT")]
    equity = np.cumprod(1 + ret)
    rows, symbols = [], []
    for peak, trough, rec in episodes(ret):
        a, b = peak + 1, trough + 1  # rows of the decline
        start_eq = equity[peak] if peak >= 0 else 1.0
        seg = ret[a:b]
        worst = np.sort(seg)[:SHOCK_DAYS]
        before = ew[max(0, a - 60) : a]
        flips = (np.diff(np.sign(sig[a - 1 : b]), axis=0) != 0).sum(axis=0)
        sym_pnl = pnl[a:b].sum(axis=0)
        order = np.argsort(sym_pnl)
        rows.append(
            {
                "peak": date(days[peak]) if peak >= 0 else date(days[0] - 1),
                "trough": date(days[trough]),
                "recovered": date(days[rec]) if rec is not None else "not yet",
                "depth": float(equity[trough] / start_eq - 1),
                "days_down": b - a,
                "days_to_recover": (rec - trough) if rec is not None else None,
                "long_pnl": float(np.where(w[a:b] > 0, pnl[a:b], 0).sum()),
                "short_pnl": float(np.where(w[a:b] < 0, pnl[a:b], 0).sum()),
                "fees": float(fees[a:b].sum()),
                "shock_share": float(worst.sum() / seg.sum()) if seg.sum() < 0 else float("nan"),
                "flips_per_symbol": float(flips.mean()),
                "btc_ret": float(np.prod(1 + btc[a:b]) - 1),
                "ew_ret": float(np.prod(1 + ew[a:b]) - 1),
                "vol_vs_prior": float(ew[a:b].std(ddof=1) / before.std(ddof=1)),
                "avg_gross": float(gross[a:b].mean()),
                "avg_vol_scale": float(scale[a:b].mean()),
                "worst_symbols": ", ".join(
                    f"{cols[j].removesuffix('USDT')} {sym_pnl[j]:+.3f}" for j in order[:3]
                ),
            },
        )
        symbols.append(sym_pnl)

    out = pl.DataFrame(rows)
    out.write_csv(vol_target.OUT_DIR_CAP / "drawdowns.csv")
    by_symbol = pl.DataFrame(
        {"symbol": cols, "loss_in_top_episodes": np.sum(symbols, axis=0)}
    ).sort("loss_in_top_episodes")
    with pl.Config(tbl_rows=50, float_precision=3, tbl_width_chars=250, fmt_str_lengths=60):
        print(out.drop("worst_symbols"))
        print(out.select("peak", "trough", "worst_symbols"))
        print(by_symbol)


if __name__ == "__main__":
    main()
