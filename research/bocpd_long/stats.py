"""
Backtest statistics: Sharpe, probabilistic and deflated Sharpe ratios (AFML ch. 14).

Sharpe ratios here are per period (daily) unless annualized explicitly.
"""

from __future__ import annotations

import math

import numpy as np
from scipy.stats import kurtosis
from scipy.stats import norm
from scipy.stats import skew


EULER_GAMMA = 0.5772156649015329


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
    return psr(r, expected_max_sharpe(trial_sharpes.shape[0], float(np.var(trial_sharpes, ddof=1))))


def max_drawdown(r: np.ndarray) -> float:
    equity = np.cumsum(r)
    return float((np.maximum.accumulate(equity) - equity).max())


def summary(r: np.ndarray, periods_per_year: int = 365) -> dict:
    sr = sharpe(r)
    return {
        "days": int(r.shape[0]),
        "mean_daily": float(r.mean()),
        "sharpe_daily": sr,
        "sharpe_annual": sr * math.sqrt(periods_per_year),
        "total_return": float(r.sum()),
        "max_drawdown": max_drawdown(r),
        "psr_0": psr(r),
    }
