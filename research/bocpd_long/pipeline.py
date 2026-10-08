"""
BOCPD-trigger long-only strategy on bpd25 dollar bars: events and full dev-period report.

Strategy:
- Events: bars where BOCPD's probability of a changepoint within the last day crosses 0.5,
  for every real symbol and its mirrored market (inverted prices, swapped taker flow).
- Labels: triple barrier at +/- 1.4 sigma (~1-day EWMA volatility) with a 3-day vertical
  barrier, checked at bar closes. Sample weights: return attribution and average uniqueness.
- Models: LightGBM primary (side) and meta (act / pass) on 22 AFML features, trained on the
  pooled real + mirrored panel with purged K-fold CV and early stopping.
- Trading: long only (primary calls up and meta approves), unhedged; per-symbol cap 5% and
  inverse-volatility sizing; exits at bar closes; taker fee 0.05% per side, no funding.

Commands:
- events: fractional-differentiation orders (dev data), the sigma reference (CUSUM events'
  sigma of real symbols) and the BOCPD events with labels, weights and features.
- report: out-of-fold models, backtest, all metrics, and CPCV path distributions
  (N=6, k=2 and N=10, k=3).

Dev period 2022-01 .. 2025-06 only; 2026 data is sealed and never read.

Usage (from research/):
    ../python/.venv/Scripts/python -m bocpd_long.pipeline events
    ../python/.venv/Scripts/python -m bocpd_long.pipeline report

"""

from __future__ import annotations

import argparse
import time

import numpy as np
import polars as pl

from bocpd_long import bars
from bocpd_long import bocpd
from bocpd_long import config
from bocpd_long import cv
from bocpd_long import events
from bocpd_long import execution
from bocpd_long import features
from bocpd_long import model
from bocpd_long import registry
from bocpd_long import report
from bocpd_long import stats
from bocpd_long import weights
from bocpd_long.features import FEATURES


CPCV_SETTINGS = ((6, 2), (10, 3))


# Events


def choose_d() -> dict[str, float]:
    found: dict[str, float | None] = {}
    for symbol in bars.real_symbols():
        df = bars.load_bars(symbol).filter(pl.col("close_time") < config.DEV_END_MS)
        found[symbol] = (
            features.min_stationary_d(np.log(df["close"].to_numpy())) if df.height else None
        )
    fallback = float(np.median([d for d in found.values() if d is not None]))
    return {s: (fallback if d is None else d) for s, d in found.items()}


def sigma_events() -> pl.DataFrame:
    """
    Sigma at real CUSUM events: the cross-sectional reference for inverse-volatility sizing.
    """
    frames = []
    for symbol in bars.real_symbols():
        ev = events.label_events(bars.load_bars(symbol))
        if ev.height:
            frames.append(ev.select("t0", "sigma").with_columns(pl.lit(symbol).alias("symbol")))
    return pl.concat(frames).sort("t0", "symbol").with_columns(pl.lit(False).alias("mirror"))


def bocpd_events(d_by_symbol: dict[str, float]) -> pl.DataFrame:
    frames = []
    for symbol, df in bars.iter_panel():
        real = symbol.removesuffix(config.MIRROR_SUFFIX)
        ev = events.label_at(df, bocpd.trigger_index(df["close"].to_numpy()))
        if ev.height == 0:
            continue
        uniq, attr = weights.uniqueness_and_attribution(
            np.log(df["close"].to_numpy()),
            ev["i0"].to_numpy(),
            ev["i1"].to_numpy(),
        )
        ev = ev.with_columns(pl.Series("uniqueness", uniq), pl.Series("attribution", attr))
        feat = features.event_features(df, events.daily_vol(df), d_by_symbol[real], ev)
        frames.append(
            feat.with_columns(
                pl.lit(symbol).alias("symbol"),
                pl.lit(real).alias("base_symbol"),
                pl.lit(bars.is_mirror(symbol)).alias("mirror"),
            ),
        )
    return pl.concat(frames).sort("t0", "symbol")


def run_events() -> None:
    started = time.time()
    config.OUT_DIR.mkdir(parents=True, exist_ok=True)
    d_by_symbol = choose_d()
    pl.DataFrame({"symbol": list(d_by_symbol), "d": list(d_by_symbol.values())}).write_csv(
        config.OUT_DIR / "ffd_d.csv",
    )
    sigma_events().write_parquet(config.OUT_DIR / "sigma_events.parquet")
    ev = bocpd_events(d_by_symbol)
    ev.write_parquet(config.OUT_DIR / "events.parquet")
    print(
        f"events: {ev.height} ({ev.filter(~pl.col('mirror')).height} real) ({time.time() - started:.0f}s)"
    )


# Report


def embargo_ms(t0: np.ndarray) -> int:
    return int(config.EMBARGO_PCT * (t0.max() - t0.min()))


def load_dev() -> pl.DataFrame:
    return (
        pl.read_parquet(config.OUT_DIR / "events.parquet")
        .filter(pl.col("t1") < config.DEV_END_MS)
        .drop_nulls(list(FEATURES))
        .sort("t0", "symbol")
    )


def fit_predict(
    dev: pl.DataFrame,
    train: np.ndarray,
    test: np.ndarray,
    p_rounds: int,
    m_rounds: int,
    emb: int,
) -> tuple[np.ndarray, np.ndarray]:
    """
    Train primary and meta models on `train` (meta on inner purged-CV primary OOF) and predict
    `test`.
    """
    tr, te = dev[train], dev[test]
    x_tr, y_tr = model.primary_xy(tr)
    w_tr = model.sample_weight(tr)
    prm = model.params(float(tr["uniqueness"].mean()))
    inner_oof = np.full(tr.height, np.nan)
    for itr, ite in cv.purged_kfold(tr["t0"].to_numpy(), tr["t1"].to_numpy(), config.N_SPLITS, emb):
        inner_oof[ite] = model.fit(x_tr[itr], y_tr[itr], w_tr[itr], prm, p_rounds).predict(
            x_tr[ite]
        )
    p_te = model.fit(x_tr, y_tr, w_tr, prm, p_rounds).predict(model.primary_xy(te)[0])
    xm_tr, ym_tr, _ = model.meta_xy(tr, inner_oof)
    meta = model.fit(xm_tr, ym_tr, w_tr, prm, m_rounds)
    return p_te, meta.predict(model.meta_xy(te, p_te)[0])


def cpcv_paths(
    dev: pl.DataFrame,
    exits: pl.DataFrame,
    sigma_ref: pl.DataFrame,
    p_rounds: int,
    m_rounds: int,
    n_groups: int,
    n_test: int,
) -> pl.DataFrame:
    t0, t1 = dev["t0"].to_numpy(), dev["t1"].to_numpy()
    emb = embargo_ms(t0)
    splits, groups, combos = cv.cpcv(t0, t1, n_groups, n_test, emb)
    paths = cv.cpcv_paths(n_groups, combos)
    preds = [
        (test, *fit_predict(dev, train, test, p_rounds, m_rounds, emb)) for train, test in splits
    ]
    rows = []
    for p in range(paths.shape[0]):
        pp = np.full(dev.height, np.nan)
        mm = np.full(dev.height, np.nan)
        for g in range(n_groups):
            test, p_te, m_te = preds[paths[p, g]]
            in_g = groups[test] == g
            pp[test[in_g]] = p_te[in_g]
            mm[test[in_g]] = m_te[in_g]
        side = np.where(pp >= 0.5, 1, -1)
        approved = np.flatnonzero((side == 1) & (execution.bet_size(mm, side) != 0))
        bets = exits.filter(pl.col("row").is_in(approved))
        daily = execution.portfolio(
            execution.size_bets(bets, sigma_ref), int(t0.min()), config.DEV_END_MS
        )
        rows.append({"path": p, "bets": bets.height} | stats.summary(daily["ret"].to_numpy()))
    return pl.DataFrame(rows)


def run_report() -> None:
    started = time.time()
    dev = load_dev()
    t0, t1 = dev["t0"].to_numpy(), dev["t1"].to_numpy()
    folds = cv.purged_kfold(t0, t1, config.N_SPLITS, embargo_ms(t0))
    w = model.sample_weight(dev)
    prm = model.params(float(dev["uniqueness"].mean()))
    real = ~dev["mirror"].to_numpy()

    x, y = model.primary_xy(dev)
    p_oof, p_rounds = model.cv_oof(x, y, w, folds, prm)
    xm, ym, side = model.meta_xy(dev, p_oof)
    m_oof, m_rounds = model.cv_oof(xm, ym, w, folds, prm)
    base = dev.with_row_index("row").select(
        "row", "symbol", "mirror", "i0", "i1", "t0", "t1", "ret", "sigma"
    )
    bets = base.with_columns(
        pl.Series("side", side), pl.Series("size", execution.bet_size(m_oof, side))
    )
    longs = bets.filter(~pl.col("mirror") & (pl.col("side") == 1))

    sigma_ref = execution.sigma_reference(pl.read_parquet(config.OUT_DIR / "sigma_events.parquet"))
    sized = execution.simulate_and_size(longs, sigma_ref)
    start, end = int(t0.min()), config.DEV_END_MS
    daily = execution.portfolio(sized, start, end)
    trial_sr = registry.sharpes().to_numpy()
    print(f"models and backtest done ({time.time() - started:.0f}s)")

    metrics = (
        report.performance(daily, trial_sr)
        | report.factor_regression(daily)
        | report.concentration(daily["ret"].to_numpy())
        | report.bet_metrics(sized, longs)
        | report.classification(y[real], p_oof[real], ym[real], m_oof[real])
        | {
            "primary_rounds": p_rounds,
            "meta_rounds": m_rounds,
            "events_real_all_sides": int(real.sum()),
            "long_calls_real": int(longs.height),
            "long_calls_share": float(longs.height / real.sum()),
            "trials_counted": int(trial_sr.size),
        }
    )

    # CPCV: exits depend only on each bet, so simulate every candidate long once
    candidates = base.filter(~pl.col("mirror")).with_columns(
        pl.lit(1).alias("side"), pl.lit(1.0).alias("size")
    )
    exits = pl.concat(
        [
            execution.simulate_exits(symbol, g)
            for (symbol,), g in candidates.sort("symbol", "t0").group_by(
                "symbol", maintain_order=True
            )
        ],
    ).filter(pl.col("exit_reason") != execution.UNRESOLVED)
    for n_groups, n_test in CPCV_SETTINGS:
        table = cpcv_paths(dev, exits, sigma_ref, p_rounds, m_rounds, n_groups, n_test)
        table.write_csv(config.OUT_DIR / f"cpcv_N{n_groups}_k{n_test}.csv")
        s = table["sharpe_annual"]
        name = f"cpcv_N{n_groups}_k{n_test}"
        metrics |= {
            f"{name}_paths": table.height,
            f"{name}_mean": float(s.mean()),
            f"{name}_median": float(s.median()),
            f"{name}_std": float(s.std()),
            f"{name}_min": float(s.min()),
            f"{name}_p10": float(s.quantile(0.1)),
            f"{name}_max": float(s.max()),
            f"{name}_share_positive": float((s > 0).mean()),
        }
        print(f"{name}: {table.height} paths ({time.time() - started:.0f}s)")

    out = pl.DataFrame({"metric": list(metrics), "value": [float(v) for v in metrics.values()]})
    out.write_csv(config.OUT_DIR / "report.csv")
    daily.write_parquet(config.OUT_DIR / "daily.parquet")
    sized.write_parquet(config.OUT_DIR / "bets.parquet")
    years = report.yearly(daily)
    years.write_csv(config.OUT_DIR / "yearly.csv")
    monthly = (
        daily.with_columns(
            pl.from_epoch(pl.col("day") * config.DAY_MS, time_unit="ms")
            .dt.strftime("%Y-%m")
            .alias("month"),
        )
        .group_by("month")
        .agg(
            ((pl.col("ret") + 1).product() - 1).alias("return"),
            pl.col("gross").mean().alias("avg_gross"),
        )
        .sort("month")
    )
    monthly.write_csv(config.OUT_DIR / "monthly.csv")
    with pl.Config(tbl_rows=200, float_precision=4, fmt_str_lengths=40):
        print(out)
        print(years)
    print(f"done ({time.time() - started:.0f}s)")


def main() -> None:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[1])
    parser.add_argument("command", choices=("events", "report"))
    args = parser.parse_args()
    run_events() if args.command == "events" else run_report()


if __name__ == "__main__":
    main()
