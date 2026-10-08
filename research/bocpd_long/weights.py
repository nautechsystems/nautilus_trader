"""
Concurrency, average uniqueness and return-attribution sample weights (AFML ch. 4).

An event spans the bar returns in (i0, i1]. Concurrency is counted per symbol.
"""

from __future__ import annotations

import numba
import numpy as np


@numba.njit(cache=True)
def uniqueness_and_attribution(log_close, i0, i1):
    """
    Return each event's average uniqueness and |sum of r_t / c_t| over its span.
    """
    n = log_close.shape[0]
    m = i0.shape[0]
    delta = np.zeros(n + 1, dtype=np.int64)
    for e in range(m):
        delta[i0[e] + 1] += 1
        delta[i1[e] + 1] -= 1
    conc = np.cumsum(delta)[:n]

    uniq = np.empty(m)
    attr = np.empty(m)
    for e in range(m):
        u = 0.0
        a = 0.0
        for t in range(i0[e] + 1, i1[e] + 1):
            u += 1.0 / conc[t]
            a += (log_close[t] - log_close[t - 1]) / conc[t]
        span = i1[e] - i0[e]
        uniq[e] = u / span
        attr[e] = abs(a)
    return uniq, attr
