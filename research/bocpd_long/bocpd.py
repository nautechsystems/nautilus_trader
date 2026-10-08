"""
Bayesian online changepoint detection (Adams & MacKay 2007) with an online-learned hazard.

Observation model: Gaussian with unknown mean and variance under a Normal-Gamma prior
(MU0, KAPPA0, ALPHA0, BETA0); the predictive density is Student-t. Run lengths are truncated
at `max_run`.

Hazard: constant in run length and learned online by soft counting. The filtered P(r_t = 0)
always equals the hazard itself, so the soft count uses a fixed lag: the average of
P(r_t = k) for k in [lag - lag_width, lag + lag_width] estimates the probability of a
changepoint about `lag` steps ago. Soft counts feed a Beta posterior on the hazard with
exponential forgetting; each step uses its mean clipped to [hazard_min, hazard_max]. Only past
observations are used.
"""

from __future__ import annotations

import math

import numba
import numpy as np
import polars as pl

from bocpd_long import config


MU0 = 0.0
KAPPA0 = 1.0
ALPHA0 = 1.0
BETA0 = 1.0


@numba.njit(cache=True)
def _student_t_logpdf(x, mu, kappa, alpha, beta):
    df = 2.0 * alpha
    scale2 = beta * (kappa + 1.0) / (alpha * kappa)
    z = (x - mu) ** 2 / (df * scale2)
    return (
        math.lgamma(0.5 * (df + 1.0))
        - math.lgamma(0.5 * df)
        - 0.5 * math.log(df * math.pi * scale2)
        - 0.5 * (df + 1.0) * math.log1p(z)
    )


@numba.njit(cache=True)
def _run_bocpd(
    x,
    fixed_hazard,
    max_run,
    recent_bars,
    shift_bars,
    lag,
    lag_width,
    prior_mean,
    prior_strength,
    memory,
    hazard_min,
    hazard_max,
):
    """
    Run BOCPD over `x` (NaN observations are skipped: features carry forward).

    `fixed_hazard` <= 0 enables the online-learned hazard. Returns per-step arrays:
    cp_recent, run_mean, run_entropy, regime_mean, regime_vol, regime_shift, hazard and the
    one-step-ahead log predictive density (NaN where x is NaN).
    """
    n = x.shape[0]
    size = max_run + 1
    prob = np.zeros(size)
    prob[0] = 1.0
    mu = np.full(size, MU0)
    kappa = np.full(size, KAPPA0)
    alpha = np.full(size, ALPHA0)
    beta = np.full(size, BETA0)
    active = 1

    a0 = prior_mean * prior_strength
    b0 = (1.0 - prior_mean) * prior_strength
    decay = 1.0 - 1.0 / memory
    soft_a = 0.0
    soft_b = 0.0

    cp_recent = np.full(n, np.nan)
    run_mean = np.full(n, np.nan)
    run_entropy = np.full(n, np.nan)
    regime_mean = np.full(n, np.nan)
    regime_vol = np.full(n, np.nan)
    regime_shift = np.full(n, np.nan)
    hazard_out = np.full(n, np.nan)
    loglik = np.full(n, np.nan)

    log_pred = np.empty(size)
    new_prob = np.empty(size)

    for t in range(n):
        xt = x[t]
        if np.isnan(xt):
            if t > 0:
                cp_recent[t] = cp_recent[t - 1]
                run_mean[t] = run_mean[t - 1]
                run_entropy[t] = run_entropy[t - 1]
                regime_mean[t] = regime_mean[t - 1]
                regime_vol[t] = regime_vol[t - 1]
                regime_shift[t] = regime_shift[t - 1]
                hazard_out[t] = hazard_out[t - 1]
            continue

        if fixed_hazard > 0.0:
            h = fixed_hazard
        else:
            h = (a0 + soft_a) / (a0 + b0 + soft_a + soft_b)
            h = min(max(h, hazard_min), hazard_max)

        # Predictive densities, growth and changepoint masses
        cp_mass = 0.0
        for r in range(active):
            log_pred[r] = _student_t_logpdf(xt, mu[r], kappa[r], alpha[r], beta[r])
        m = -np.inf
        for r in range(active):
            if prob[r] > 0.0 and log_pred[r] > m:
                m = log_pred[r]
        for r in range(size):
            new_prob[r] = 0.0
        for r in range(active):
            w = prob[r] * math.exp(log_pred[r] - m)
            cp_mass += w * h
            dest = r + 1 if r + 1 < size else size - 1
            new_prob[dest] += w * (1.0 - h)
        new_prob[0] += cp_mass
        z = 0.0
        for r in range(size):
            z += new_prob[r]
        loglik[t] = m + math.log(z)
        for r in range(size):
            new_prob[r] /= z

        # Sufficient statistics: shift by one run length, reset r = 0 to the prior
        # At the truncation limit the last bucket takes the next-oldest run's statistics
        last = min(active, size - 1)
        for r in range(last, 0, -1):
            k = kappa[r - 1]
            mu_old = mu[r - 1]
            beta[r] = beta[r - 1] + k * (xt - mu_old) ** 2 / (2.0 * (k + 1.0))
            mu[r] = (k * mu_old + xt) / (k + 1.0)
            kappa[r] = k + 1.0
            alpha[r] = alpha[r - 1] + 0.5
        mu[0] = MU0
        kappa[0] = KAPPA0
        alpha[0] = ALPHA0
        beta[0] = BETA0
        active = min(active + 1, size)
        for r in range(size):
            prob[r] = new_prob[r]

        # Hazard soft counts: changepoint probability about LAG steps ago
        if active > lag + lag_width:
            c = 0.0
            for r in range(lag - lag_width, lag + lag_width + 1):
                c += prob[r]
            c /= 2 * lag_width + 1
            soft_a = decay * soft_a + c
            soft_b = decay * soft_b + (1.0 - c)

        # Features
        recent = 0.0
        rmean = 0.0
        ent = 0.0
        mmean = 0.0
        vmean = 0.0
        best = 0
        for r in range(active):
            p = prob[r]
            if r < recent_bars:
                recent += p
            rmean += r * p
            if p > 0.0:
                ent -= p * math.log(p)
            mmean += p * mu[r]
            vmean += p * math.sqrt(beta[r] / alpha[r])
            if p > prob[best]:
                best = r
        cp_recent[t] = recent
        run_mean[t] = rmean
        run_entropy[t] = ent
        regime_mean[t] = mmean
        regime_vol[t] = vmean
        hazard_out[t] = h

        start = t - best
        lo = max(0, start - shift_bars)
        acc = 0.0
        cnt = 0
        for s in range(lo, start):
            if not np.isnan(x[s]):
                acc += x[s]
                cnt += 1
        if cnt >= 10:
            regime_shift[t] = mu[best] - acc / cnt

    return (
        cp_recent,
        run_mean,
        run_entropy,
        regime_mean,
        regime_vol,
        regime_shift,
        hazard_out,
        loglik,
    )


def changepoint_probability(close: np.ndarray) -> np.ndarray:
    """
    Probability of a changepoint within the last BOCPD_RECENT_BARS bars, per bar.

    Input: bar log returns standardized by their EWMA std (span VOL_SPAN) up to t-1.
    """
    x = (
        pl.DataFrame({"close": close})
        .select(pl.col("close").log().diff().alias("r"))
        .with_columns(
            (
                pl.col("r")
                / pl.col("r").ewm_std(span=config.VOL_SPAN, min_samples=config.VOL_SPAN).shift(1)
            ).alias("x"),
        )["x"]
        .fill_null(np.nan)
        .fill_nan(np.nan)
        .to_numpy()
    )
    # Settings are passed explicitly: numba freezes globals at compile time
    cp = _run_bocpd(
        x,
        0.0,
        config.BOCPD_MAX_RUN,
        config.BOCPD_RECENT_BARS,
        config.BOCPD_SHIFT_BARS,
        config.BOCPD_LAG,
        config.BOCPD_LAG_WIDTH,
        config.BOCPD_HAZARD_PRIOR_MEAN,
        config.BOCPD_HAZARD_PRIOR_STRENGTH,
        config.BOCPD_HAZARD_MEMORY,
        config.BOCPD_HAZARD_MIN,
        config.BOCPD_HAZARD_MAX,
    )[0]
    return np.where(np.isfinite(cp), cp, 0.0)


def trigger_index(close: np.ndarray) -> np.ndarray:
    """
    Bars where the changepoint probability crosses up through BOCPD_TRIGGER.
    """
    cp = changepoint_probability(close)
    prev = np.concatenate([[1.0], cp[:-1]])
    return np.flatnonzero((cp >= config.BOCPD_TRIGGER) & (prev < config.BOCPD_TRIGGER))
