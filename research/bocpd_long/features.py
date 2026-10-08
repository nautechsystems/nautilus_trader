"""
AFML features computed per symbol from bars up to and including the event bar.

Per-bar features use trailing windows only. SADF and entropy are computed at event bars.
Features that depend on direction (fractional differentiation, order-flow imbalance, SADF,
CUSUM direction) change under the mirrored market; the rest are invariant by construction.
"""

from __future__ import annotations

import math

import numba
import numpy as np
import polars as pl
from statsmodels.tsa.stattools import adfuller

from bocpd_long import config


FEATURES = (
    "ffd_z",
    "sadf",
    "cusum_dir",
    "cusum_overshoot",
    "lz_entropy",
    "konto_entropy",
    "roll_spread",
    "cs_spread",
    "bp_sigma",
    "kyle_lambda",
    "amihud_z",
    "vpin",
    "ofi_short",
    "ofi_long",
    "sigma",
    "vol_ratio",
    "vol_change",
    "gk_ratio",
    "n_minutes_log",
    "n_minutes_short",
    "n_minutes_z",
    "n_minutes_change",
)


# Fractional differentiation (AFML ch. 5)


def ffd_weights(d: float, threshold: float = config.FFD_THRESHOLD) -> np.ndarray:
    w = [1.0]
    k = 1
    while True:
        nxt = -w[-1] * (d - k + 1) / k
        if abs(nxt) < threshold:
            break
        w.append(nxt)
        k += 1
    return np.array(w)


@numba.njit(cache=True)
def _ffd(x, w):
    n = x.shape[0]
    width = w.shape[0]
    out = np.full(n, np.nan)
    for i in range(width - 1, n):
        acc = 0.0
        for k in range(width):
            acc += w[k] * x[i - k]
        out[i] = acc
    return out


def ffd(log_close: np.ndarray, d: float) -> np.ndarray:
    return _ffd(log_close, ffd_weights(d))


def min_stationary_d(log_close: np.ndarray) -> float | None:
    """
    Smallest d on the grid whose FFD series passes the ADF test, or None if too short.
    """
    for d in config.FFD_D_GRID:
        series = ffd(log_close, d)
        series = series[~np.isnan(series)]
        if series.shape[0] < 500:
            return None
        pvalue = adfuller(series, maxlag=1, regression="c", autolag=None, result_object=False)[1]
        if pvalue < config.ADF_PVALUE:
            return d
    return 1.0


# SADF (AFML ch. 17), evaluated at event bars


@numba.njit(cache=True)
def _df_tstat(y):
    # Dickey-Fuller regression dy_t = a + b * y_{t-1} + e_t; returns the t-stat of b
    n = y.shape[0] - 1
    sx = 0.0
    sy = 0.0
    sxx = 0.0
    sxy = 0.0
    for t in range(n):
        x = y[t]
        dy = y[t + 1] - y[t]
        sx += x
        sy += dy
        sxx += x * x
        sxy += x * dy
    mx = sx / n
    my = sy / n
    vxx = sxx - n * mx * mx
    if vxx <= 0.0:
        return np.nan
    b = (sxy - n * mx * my) / vxx
    a = my - b * mx
    sse = 0.0
    for t in range(n):
        e = (y[t + 1] - y[t]) - a - b * y[t]
        sse += e * e
    s2 = sse / (n - 2)
    if s2 <= 0.0:
        return np.nan
    return b / math.sqrt(s2 / vxx)


@numba.njit(cache=True)
def sadf_at(log_close, idx, window, min_bars, step):
    out = np.full(idx.shape[0], np.nan)
    for e in range(idx.shape[0]):
        end = idx[e] + 1
        begin = end - window
        if begin < 0:
            continue
        best = -np.inf
        for s in range(begin, end - min_bars + 1, step):
            t = _df_tstat(log_close[s:end])
            if not np.isnan(t) and t > best:
                best = t
        if best > -np.inf:
            out[e] = best
    return out


# Entropy (AFML ch. 18), evaluated at event bars on a ternary return encoding


@numba.njit(cache=True)
def _lz76(s):
    # Kaspar-Schuster LZ76 complexity, normalized by n / log2(n)
    n = s.shape[0]
    c = 1
    i = 0
    k = 1
    k_max = 1
    l = 1
    while True:
        if s[i + k - 1] == s[l + k - 1]:
            k += 1
            if l + k > n:
                c += 1
                break
        else:
            k_max = max(k, k_max)
            i += 1
            if i == l:
                c += 1
                l += k_max
                if l + 1 > n:
                    break
                i = 0
                k = 1
                k_max = 1
            else:
                k = 1
    return c * math.log2(n) / n


@numba.njit(cache=True)
def _match_length(s, i, n):
    # Longest l such that s[i:i+l] occurs starting in s[i-n:i]; returns l + 1 (AFML 18.3)
    best = 0
    length = s.shape[0]
    for l in range(1, n + 1):
        if i + l > length:
            break
        found = False
        for j in range(i - n, i):
            ok = True
            for q in range(l):
                if s[j + q] != s[i + q]:
                    ok = False
                    break
            if ok:
                found = True
                break
        if not found:
            break
        best = l
    return best + 1


@numba.njit(cache=True)
def _kontoyiannis(s):
    # Expanding-window Kontoyiannis entropy rate estimate (AFML snippet 18.4)
    n = s.shape[0]
    total = 0.0
    count = 0
    for i in range(1, n // 2 + 1):
        total += math.log2(i + 1) / _match_length(s, i, i)
        count += 1
    return total / count


@numba.njit(cache=True)
def entropy_at(log_close, idx, window):
    lz = np.full(idx.shape[0], np.nan)
    kt = np.full(idx.shape[0], np.nan)
    for e in range(idx.shape[0]):
        end = idx[e] + 1
        begin = end - window - 1
        if begin < 0:
            continue
        r = np.diff(log_close[begin:end])
        code = np.where(r > 0.0, 2, np.where(r < 0.0, 0, 1))
        lz[e] = _lz76(code)
        kt[e] = _kontoyiannis(code)
    return lz, kt


# Per-bar features


def _zscore(expr: pl.Expr, window: int) -> pl.Expr:
    mean = expr.rolling_mean(window, min_samples=window)
    std = expr.rolling_std(window, min_samples=window)
    return (expr - mean) / std


def bar_features(bars: pl.DataFrame, sigma: np.ndarray, d: float) -> pl.DataFrame:
    """
    Trailing-window features for every bar of one symbol.
    """
    log_close = np.log(bars["close"].to_numpy())
    zw, mw, sw = config.ZSCORE_BARS, config.MICRO_BARS, config.SHORT_BARS
    k2 = math.sqrt(8.0 / math.pi)
    den = 3.0 - 2.0 * math.sqrt(2.0)

    r = pl.col("r")
    hl2 = (pl.col("high") / pl.col("low")).log() ** 2
    beta = hl2 + hl2.shift(1)
    gamma = (
        pl.max_horizontal(pl.col("high"), pl.col("high").shift(1))
        / pl.min_horizontal(pl.col("low"), pl.col("low").shift(1))
    ).log() ** 2
    alpha = ((2 * beta).sqrt() - beta.sqrt()) / den - (gamma / den).sqrt()
    alpha = pl.when(alpha < 0).then(0.0).otherwise(alpha)
    cs = 2 * (alpha.exp() - 1) / (1 + alpha.exp())
    bp = (2**-0.5 - 1) * beta.sqrt() / (k2 * den) + (gamma / (k2**2 * den)).sqrt()
    bp = pl.when(bp < 0).then(0.0).otherwise(bp)
    gk = 0.5 * hl2 - (2 * math.log(2) - 1) * (pl.col("close") / pl.col("open")).log() ** 2
    ofi = (2 * pl.col("taker_buy_quote_volume") - pl.col("quote_volume")) / pl.col("quote_volume")
    cov_lag = (r * r.shift(1)).rolling_mean(mw) - r.rolling_mean(mw) * r.shift(1).rolling_mean(mw)
    kyle = (
        (r * pl.col("ofi")).rolling_mean(mw) - r.rolling_mean(mw) * pl.col("ofi").rolling_mean(mw)
    ) / pl.col(
        "ofi",
    ).rolling_var(mw, ddof=0)
    amihud = (r.abs().rolling_mean(mw) / pl.col("quote_volume").rolling_mean(mw)).log()
    nm = pl.col("n_minutes").cast(pl.Float64).log()

    return (
        bars.with_columns(
            pl.Series("r", np.concatenate([[np.nan], np.diff(log_close)])).fill_nan(None),
            pl.Series("ffd", ffd(log_close, d)).fill_nan(None),
            pl.Series("sigma", sigma).fill_nan(None),
            ofi.alias("ofi"),
        )
        .with_columns(
            _zscore(pl.col("ffd"), zw).alias("ffd_z"),
            (2 * pl.when(cov_lag < 0).then(-cov_lag).otherwise(0.0).sqrt()).alias("roll_spread"),
            cs.rolling_mean(mw).alias("cs_spread"),
            bp.rolling_mean(mw).alias("bp_sigma"),
            kyle.alias("kyle_lambda"),
            _zscore(amihud, zw).alias("amihud_z"),
            pl.col("ofi").abs().rolling_mean(mw).alias("vpin"),
            pl.col("ofi").rolling_mean(sw).alias("ofi_short"),
            pl.col("ofi").rolling_mean(mw).alias("ofi_long"),
            (pl.col("sigma") / pl.col("sigma").rolling_mean(zw)).alias("vol_ratio"),
            (pl.col("sigma") / pl.col("sigma").shift(sw)).log().alias("vol_change"),
            (gk.rolling_mean(sw).sqrt() / gk.rolling_mean(mw).sqrt()).alias("gk_ratio"),
            nm.alias("n_minutes_log"),
            nm.rolling_mean(sw).alias("n_minutes_short"),
            _zscore(nm.rolling_mean(sw), zw).alias("n_minutes_z"),
            (nm.rolling_mean(sw) - nm.rolling_mean(mw)).alias("n_minutes_change"),
        )
        .with_columns(pl.selectors.float().fill_nan(None))
    )


def event_features(
    bars: pl.DataFrame, sigma: np.ndarray, d: float, ev: pl.DataFrame
) -> pl.DataFrame:
    """
    Feature matrix for the events of one symbol (rows aligned with `ev`).
    """
    log_close = np.log(bars["close"].to_numpy())
    idx = ev["i0"].to_numpy()
    per_bar = bar_features(bars, sigma, d)
    sadf = sadf_at(log_close, idx, config.SADF_WINDOW, config.SADF_MIN_BARS, config.SADF_STEP)
    lz, kt = entropy_at(log_close, idx, config.ENTROPY_BARS)
    cols = [c for c in FEATURES if c in per_bar.columns and c != "sigma"]
    return pl.concat(
        [
            ev,
            per_bar[idx].select(cols),
            pl.DataFrame({"sadf": sadf, "lz_entropy": lz, "konto_entropy": kt}),
        ],
        how="horizontal_extend",
    ).with_columns(pl.selectors.float().fill_nan(None))
