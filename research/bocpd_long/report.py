"""
Backtest statistics (AFML ch. 14), factor regression, return concentration, bet-level and
classification metrics.
"""

from __future__ import annotations

import math

import numpy as np
import polars as pl
import statsmodels.api as sm
from scipy.stats import kurtosis
from scipy.stats import skew
from sklearn.metrics import accuracy_score
from sklearn.metrics import f1_score
from sklearn.metrics import log_loss
from sklearn.metrics import precision_score
from sklearn.metrics import recall_score
from sklearn.metrics import roc_auc_score

from bocpd_long import bars
from bocpd_long import config
from bocpd_long import execution
from bocpd_long import stats


HEDGE_SYMBOL = "BTCUSDT"


def hhi(r: np.ndarray) -> float:
    """
    Normalized Herfindahl concentration of returns (AFML snippet 14.3).
    """
    if r.shape[0] <= 2:
        return float("nan")
    w = r / r.sum()
    h = (w**2).sum()
    return float((h - 1 / r.shape[0]) / (1 - 1 / r.shape[0]))


def drawdown_runs(r: np.ndarray) -> tuple[np.ndarray, np.ndarray]:
    """
    Drawdowns (fraction of the prior peak) and their durations in days, per episode.
    """
    equity = np.cumprod(1 + r)
    peak = np.maximum.accumulate(equity)
    dd, tuw = [], []
    start, depth = None, 0.0
    for i, (e, p) in enumerate(zip(equity, peak, strict=True)):
        if e < p:
            if start is None:
                start, depth = i, 0.0
            depth = max(depth, 1 - e / p)
        elif start is not None:
            dd.append(depth)
            tuw.append(i - start)
            start = None
    if start is not None:
        dd.append(depth)
        tuw.append(len(r) - start)
    return np.array(dd), np.array(tuw)


def market_betas(daily: pl.DataFrame) -> dict:
    """
    HAC-robust betas of daily strategy returns on the equal-weight real market and on BTC.
    """
    ew = (
        pl.concat([execution.daily_returns(s) for s in bars.real_symbols()])
        .group_by("day")
        .agg(pl.col("r").mean().alias("ew"))
    )
    btc = execution.daily_returns(HEDGE_SYMBOL).rename({"r": "btc"})
    d = (
        daily.join(ew, on="day", how="left")
        .join(btc, on="day", how="left")
        .with_columns(pl.col("ew", "btc").fill_null(0.0))
    )
    out = {}
    for name in ("ew", "btc"):
        fit = sm.OLS(d["ret"].to_numpy(), sm.add_constant(d[name].to_numpy())).fit(
            cov_type="HAC",
            cov_kwds={"maxlags": 5},
        )
        out[f"beta_{name}"] = float(fit.params[1])
        out[f"beta_{name}_t"] = float(fit.tvalues[1])
        out[f"alpha_bp_vs_{name}"] = float(fit.params[0] * 1e4)
        out[f"alpha_t_vs_{name}"] = float(fit.tvalues[0])
    return out


def performance(daily: pl.DataFrame, trial_sr: np.ndarray) -> dict:
    r = daily["ret"].to_numpy()
    cap = np.maximum(1.0, daily["gross"].to_numpy())
    fee_ret = config.FEE * daily["turnover"].to_numpy() / cap
    years = r.shape[0] / 365
    equity = np.cumprod(1 + r)
    dd, tuw = drawdown_runs(r)
    down = r[r < 0]
    pos, neg = r[r > 0], r[r < 0]
    monthly = (
        daily.with_columns(
            pl.from_epoch(pl.col("day") * config.DAY_MS, time_unit="ms")
            .dt.strftime("%Y-%m")
            .alias("m")
        )
        .group_by("m")
        .agg(pl.col("ret").sum())["ret"]
        .to_numpy()
    )
    sr = stats.sharpe(r)
    return {
        # General
        "days": r.shape[0],
        "avg_leverage": float((daily["gross"].to_numpy() / cap).mean()),
        "max_leverage": float((daily["gross"].to_numpy() / cap).max()),
        "days_with_exposure": float((daily["gross"] > 0).mean()),
        "active_symbols_per_day": float(daily["active_symbols"].mean()),
        "position_changes_per_day": float(daily["trades"].mean()),
        "annual_equity_turnover": float((daily["turnover"].to_numpy() / cap).mean() * 365),
        # Performance
        "total_return_simple": float(r.sum()),
        "total_return_compounded": float(equity[-1] - 1),
        "cagr": float(equity[-1] ** (1 / years) - 1),
        "annual_return_arith": float(r.mean() * 365),
        "annual_volatility": float(r.std(ddof=1) * math.sqrt(365)),
        "daily_hit_ratio": float((r > 0).mean() / max(1e-12, (r != 0).mean())),
        "avg_up_day": float(pos.mean()) if pos.size else 0.0,
        "avg_down_day": float(neg.mean()) if neg.size else 0.0,
        "best_day": float(r.max()),
        "worst_day": float(r.min()),
        "profit_factor_daily": float(pos.sum() / -neg.sum()) if neg.size else float("inf"),
        "skew": float(skew(r)),
        "excess_kurtosis": float(kurtosis(r)),
        "positive_months_share": float((monthly > 0).mean()),
        # Runs
        "hhi_positive": hhi(pos),
        "hhi_negative": hhi(neg),
        "hhi_monthly": hhi(np.abs(monthly)),
        "max_drawdown": float(dd.max()) if dd.size else 0.0,
        "drawdown_p95": float(np.percentile(dd, 95)) if dd.size else 0.0,
        "max_time_under_water_days": int(tuw.max()) if tuw.size else 0,
        "time_under_water_p95_days": float(np.percentile(tuw, 95)) if tuw.size else 0.0,
        "calmar": float((equity[-1] ** (1 / years) - 1) / dd.max()) if dd.size else float("inf"),
        # Efficiency
        "sharpe_annual": sr * math.sqrt(365),
        "sortino_annual": float(
            r.mean() / math.sqrt((np.minimum(r, 0) ** 2).mean()) * math.sqrt(365)
        )
        if down.size
        else float("inf"),
        "psr_0": stats.psr(r),
        "dsr": stats.dsr(r, trial_sr),
        "trials_counted": int(trial_sr.size),
        "expected_max_sharpe_annual": stats.expected_max_sharpe(
            trial_sr.size, float(np.var(trial_sr, ddof=1))
        )
        * math.sqrt(365),
        # Costs
        "annual_fee_drag": float(fee_ret.sum() / years),
        "gross_annual_return_arith": float((r + fee_ret).mean() * 365),
        "fees_share_of_gross_pnl": float(fee_ret.sum() / (r + fee_ret).sum()),
        "return_per_unit_turnover_bp": float(
            r.sum() / (daily["turnover"].to_numpy() / cap).sum() * 1e4,
        ),
    } | market_betas(daily)


def yearly(daily: pl.DataFrame) -> pl.DataFrame:
    rows = []
    d = daily.with_columns(
        pl.from_epoch(pl.col("day") * config.DAY_MS, time_unit="ms").dt.year().alias("year")
    )
    for (year,), g in d.group_by("year", maintain_order=True):
        r = g["ret"].to_numpy()
        dd, _ = drawdown_runs(r)
        rows.append(
            {
                "year": year,
                "days": r.shape[0],
                "return_compounded": float(np.prod(1 + r) - 1),
                "sharpe_annual": stats.sharpe(r) * math.sqrt(365),
                "max_drawdown": float(dd.max()) if dd.size else 0.0,
                "hit_days": float((r > 0).mean()),
            },
        )
    return pl.DataFrame(rows).sort("year")


def factor_regression(daily: pl.DataFrame) -> dict:
    ew = (
        pl.concat([execution.daily_returns(s) for s in bars.real_symbols()])
        .group_by("day")
        .agg(pl.col("r").mean().alias("ew"))
    )
    btc = execution.daily_returns(HEDGE_SYMBOL).rename({"r": "btc"})
    d = (
        daily.join(ew, on="day", how="left")
        .join(btc, on="day", how="left")
        .with_columns(pl.col("ew", "btc").fill_null(0.0))
        .with_columns((pl.col("ew") - pl.col("btc")).alias("alt_minus_btc"))
    )
    x = sm.add_constant(d.select("btc", "alt_minus_btc").to_numpy())
    fit = sm.OLS(d["ret"].to_numpy(), x).fit(cov_type="HAC", cov_kwds={"maxlags": 5})
    resid = d["ret"].to_numpy() - x[:, 1:] @ fit.params[1:]
    return {
        "ff_beta_btc": float(fit.params[1]),
        "ff_beta_btc_t": float(fit.tvalues[1]),
        "ff_beta_alt_minus_btc": float(fit.params[2]),
        "ff_beta_alt_minus_btc_t": float(fit.tvalues[2]),
        "ff_alpha_bp": float(fit.params[0] * 1e4),
        "ff_alpha_t": float(fit.tvalues[0]),
        "ff_r2": float(fit.rsquared),
        "ff_neutral_sharpe_annual": stats.sharpe(resid) * math.sqrt(365),
    }


def concentration(r: np.ndarray) -> dict:
    srt = np.sort(r)[::-1]
    out = {}
    for k in (1, 5, 10, 20):
        out[f"top{k}_days_share_of_total"] = float(srt[:k].sum() / r.sum())
        rest = np.sort(r)[: r.shape[0] - k]
        out[f"sharpe_annual_without_top{k}"] = stats.sharpe(rest) * math.sqrt(365)
    return out


def _share_positive(x: pl.Series) -> float:
    return float((x > 0).mean()) if x.len() else float("nan")


def bet_metrics(sized: pl.DataFrame, all_bets: pl.DataFrame) -> dict:
    real = sized.filter(~pl.col("mirror"))
    gross = real["side"] * (real["exit_px"] / real["entry_px"] - 1)
    net = gross - 2 * config.FEE
    pnl_equity = real["weight"] * (real["exit_px"] / real["entry_px"] - 1)
    hold = (real["exit_t"] - real["t0"]) / config.DAY_MS
    hits, misses = net.filter(net > 0), net.filter(net <= 0)
    return {
        "events_real": int((~all_bets["mirror"]).sum()),
        "bets_approved_real": int(((all_bets["size"] != 0) & ~all_bets["mirror"]).sum()),
        "bets_traded_real": real.height,
        "symbols_traded": real["symbol"].n_unique(),
        "long_share_traded": float((real["side"] == 1).mean()),
        "avg_abs_weight": float(real["weight"].abs().mean()),
        "max_abs_weight": float(real["weight"].abs().max()),
        "hold_days_median": float(hold.median()),
        "hold_days_mean": float(hold.mean()),
        "exit_profit_share": float((real["exit_reason"] == execution.PROFIT).mean()),
        "exit_loss_share": float((real["exit_reason"] == execution.LOSS_AT_CLOSE).mean()),
        "exit_vertical_share": float((real["exit_reason"] == execution.VERTICAL).mean()),
        "bet_hit_ratio_net": float((net > 0).mean()),
        "bet_avg_return_net": float(net.mean()),
        "bet_avg_hit_net": float(hits.mean()),
        "bet_avg_miss_net": float(misses.mean()),
        "bet_payoff_ratio": float(hits.mean() / -misses.mean()),
        "long_hit_ratio_net": _share_positive(net.filter(real["side"] == 1)),
        "short_hit_ratio_net": _share_positive(net.filter(real["side"] == -1)),
        "best_bet_pnl_of_equity": float(pnl_equity.max()),
        "worst_bet_pnl_of_equity": float(pnl_equity.min()),
    }


def classification(y: np.ndarray, p: np.ndarray, ym: np.ndarray, pm: np.ndarray) -> dict:
    out = {}
    for name, yt, pp in (("primary", y, p), ("meta", ym, pm)):
        yhat = (pp >= 0.5).astype(int)
        n = yt.shape[0]
        auc = roc_auc_score(yt, pp)
        q1, q2 = auc / (2 - auc), 2 * auc**2 / (1 + auc)
        n1, n0 = yt.sum(), n - yt.sum()
        se = math.sqrt(
            (auc * (1 - auc) + (n1 - 1) * (q1 - auc**2) + (n0 - 1) * (q2 - auc**2)) / (n1 * n0)
        )
        out |= {
            f"{name}_n": n,
            f"{name}_accuracy": accuracy_score(yt, yhat),
            f"{name}_precision": precision_score(yt, yhat, zero_division=0),
            f"{name}_recall": recall_score(yt, yhat, zero_division=0),
            f"{name}_f1": f1_score(yt, yhat, zero_division=0),
            f"{name}_neg_logloss": -log_loss(yt, pp),
            f"{name}_auc": auc,
            f"{name}_auc_se": se,
            f"{name}_base_rate": float(yt.mean()),
        }
    return out
