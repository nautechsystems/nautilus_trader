"""
Shared tools for the trend research: sealed 1-minute kline loading, the trial registry, and
backtest statistics (AFML ch. 14) with performance, factor and concentration reports.
"""

from __future__ import annotations

import math
import re
from datetime import UTC
from datetime import datetime
from pathlib import Path

import numpy as np
import polars as pl
import statsmodels.api as sm
from scipy.stats import kurtosis
from scipy.stats import norm
from scipy.stats import skew


ROOT = Path(__file__).resolve().parents[2]
KLINES_DIR = ROOT / "binance_data" / "klines_1m"
OUT_DIR = ROOT / "binance_data" / "trend"
REGISTRY_PATH = OUT_DIR / "trial_registry.csv"
DAY_MS = 86_400_000
FEE = 0.0005  # taker fee per side, no slippage

# Data from SEAL_START_MS on is sealed and never loaded (2026-H1 was unsealed on 2026-10-13)
SEAL_START_MS = int(datetime(2026, 7, 1, tzinfo=UTC).timestamp() * 1000)
_SEAL = datetime.fromtimestamp(SEAL_START_MS / 1000, UTC)
SEAL_MONTH = (_SEAL.year, _SEAL.month)
_MONTH_RE = re.compile(r"-1m-(\d{4})-(\d{2})\.parquet$")
EULER_GAMMA = 0.5772156649015329


# Data


def unsealed_files(symbol: str) -> list[Path]:
    """
    Monthly kline files before the seal month, selected by name before anything is read.
    """
    out = []
    for path in sorted((KLINES_DIR / symbol).glob("*.parquet")):
        m = _MONTH_RE.search(path.name)
        if m is None:
            raise ValueError(f"Unexpected file name: {path}")
        if (int(m[1]), int(m[2])) < SEAL_MONTH:
            out.append(path)
    return out


def load_minutes(symbol: str) -> pl.DataFrame:
    """
    1-minute bars (open_time, close_time, high, low, close) before the seal, sorted.
    """
    return (
        pl.read_parquet(
            unsealed_files(symbol), columns=["open_time", "close_time", "high", "low", "close"]
        )
        .filter(pl.col("close_time") < SEAL_START_MS)
        .with_columns(pl.col("high", "low", "close").cast(pl.Float64))
        .sort("open_time")
        .unique("open_time", keep="first", maintain_order=True)
    )


# Trial registry


def load_registry() -> pl.DataFrame:
    if not REGISTRY_PATH.exists():
        return pl.DataFrame(schema={"name": pl.String, "sharpe_daily": pl.Float64})
    return pl.read_csv(REGISTRY_PATH, schema={"name": pl.String, "sharpe_daily": pl.Float64})


def register(name: str, sharpe_daily: float) -> None:
    """
    Record a variant's daily Sharpe; re-running a variant updates its row. Never delete rows:
    the deflated Sharpe ratio depends on every trial evaluated.
    """
    reg = load_registry().filter(pl.col("name") != name)
    reg = pl.concat([reg, pl.DataFrame({"name": [name], "sharpe_daily": [sharpe_daily]})])
    REGISTRY_PATH.parent.mkdir(parents=True, exist_ok=True)
    reg.write_csv(REGISTRY_PATH)


# Statistics (Sharpe ratios per day unless annualized)


def sharpe(r: np.ndarray) -> float:
    sd = r.std(ddof=1)
    return float(r.mean() / sd) if sd > 0 else 0.0


def psr(r: np.ndarray, sr_benchmark: float = 0.0) -> float:
    """
    Probability that the true Sharpe exceeds `sr_benchmark` given non-normal returns.
    """
    sr = sharpe(r)
    g3 = skew(r)
    g4 = kurtosis(r, fisher=False)
    denom = math.sqrt(max(1e-12, 1 - g3 * sr + (g4 - 1) / 4 * sr**2))
    return float(norm.cdf((sr - sr_benchmark) * math.sqrt(r.shape[0] - 1) / denom))


def expected_max_sharpe(n_trials: int, var_sharpe: float) -> float:
    """
    Expected maximum Sharpe of `n_trials` unskilled trials (False Strategy Theorem).
    """
    if n_trials < 2 or var_sharpe <= 0:
        return 0.0
    return math.sqrt(var_sharpe) * (
        (1 - EULER_GAMMA) * norm.ppf(1 - 1 / n_trials)
        + EULER_GAMMA * norm.ppf(1 - 1 / (n_trials * math.e))
    )


def dsr(r: np.ndarray, trial_sharpes: np.ndarray) -> float:
    var = float(np.var(trial_sharpes, ddof=1)) if trial_sharpes.shape[0] > 1 else 0.0
    return psr(r, expected_max_sharpe(trial_sharpes.shape[0], var))


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


# Reports


def performance(daily: pl.DataFrame, trial_sr: np.ndarray) -> dict:
    """
    Daily portfolio metrics; `daily` has day, ret, gross, turnover, trades and active_symbols.
    """
    r = daily["ret"].to_numpy()
    cap = np.maximum(1.0, daily["gross"].to_numpy())
    fee_ret = FEE * daily["turnover"].to_numpy() / cap
    years = r.shape[0] / 365
    equity = np.cumprod(1 + r)
    dd, tuw = drawdown_runs(r)
    down = r[r < 0]
    pos, neg = r[r > 0], r[r < 0]
    monthly = (
        daily.with_columns(
            pl.from_epoch(pl.col("day") * DAY_MS, time_unit="ms").dt.strftime("%Y-%m").alias("m")
        )
        .group_by("m")
        .agg(pl.col("ret").sum())["ret"]
        .to_numpy()
    )
    sr = sharpe(r)
    var_trials = float(np.var(trial_sr, ddof=1)) if trial_sr.size > 1 else 0.0
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
        "psr_0": psr(r),
        "dsr": dsr(r, trial_sr),
        "trials_counted": int(trial_sr.size),
        "expected_max_sharpe_annual": expected_max_sharpe(trial_sr.size, var_trials)
        * math.sqrt(365),
        # Costs
        "annual_fee_drag": float(fee_ret.sum() / years),
        "gross_annual_return_arith": float((r + fee_ret).mean() * 365),
        "fees_share_of_gross_pnl": float(fee_ret.sum() / (r + fee_ret).sum()),
        "return_per_unit_turnover_bp": float(
            r.sum() / (daily["turnover"].to_numpy() / cap).sum() * 1e4,
        ),
    }


def yearly(daily: pl.DataFrame) -> pl.DataFrame:
    rows = []
    d = daily.with_columns(
        pl.from_epoch(pl.col("day") * DAY_MS, time_unit="ms").dt.year().alias("year")
    )
    for (year,), g in d.group_by("year", maintain_order=True):
        r = g["ret"].to_numpy()
        dd, _ = drawdown_runs(r)
        rows.append(
            {
                "year": year,
                "days": r.shape[0],
                "return_compounded": float(np.prod(1 + r) - 1),
                "sharpe_annual": sharpe(r) * math.sqrt(365),
                "max_drawdown": float(dd.max()) if dd.size else 0.0,
                "hit_days": float((r > 0).mean()),
            },
        )
    return pl.DataFrame(rows).sort("year")


def factor_regression(daily: pl.DataFrame, market: pl.DataFrame) -> dict:
    """
    HAC regression of daily returns on BTC and on the equal-weight market minus BTC.

    `market` has day, btc and ew (daily simple returns realized over that day).
    """
    d = (
        daily.join(market, on="day", how="left")
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
        "ff_neutral_sharpe_annual": sharpe(resid) * math.sqrt(365),
    }


def concentration(r: np.ndarray) -> dict:
    srt = np.sort(r)[::-1]
    out = {}
    for k in (1, 5, 10, 20):
        out[f"top{k}_days_share_of_total"] = float(srt[:k].sum() / r.sum())
        rest = np.sort(r)[: r.shape[0] - k]
        out[f"sharpe_annual_without_top{k}"] = sharpe(rest) * math.sqrt(365)
    return out
