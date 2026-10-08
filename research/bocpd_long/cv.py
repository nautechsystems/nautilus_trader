"""
Purged K-fold and combinatorial purged cross-validation over event intervals (AFML ch. 7, 12).

Events from all symbols (real and mirrored) are split on time. Fold boundaries fall between
distinct t0 values, so events sharing a timestamp (such as an event and its mirror) always
land in the same fold. A training event is purged when its [t0, t1] overlaps a test group's
span, and embargoed when it starts within `embargo_ms` after that span ends. Purging is
applied across symbols because crypto returns are strongly correlated.
"""

from __future__ import annotations

from itertools import combinations

import numpy as np


def time_groups(t0: np.ndarray, n_groups: int) -> np.ndarray:
    """
    Assign each event to one of `n_groups` contiguous time groups (by distinct t0).
    """
    distinct = np.unique(t0)
    edges = distinct[np.linspace(0, distinct.shape[0], n_groups + 1).astype(int)[1:-1]]
    return np.searchsorted(edges, t0, side="right")


def train_mask(
    t0: np.ndarray, t1: np.ndarray, test: np.ndarray, groups: np.ndarray, embargo_ms: int
) -> np.ndarray:
    """
    Events usable for training given a boolean test mask: not in test, not overlapping any
    test group's span and not inside the embargo after it.
    """
    keep = ~test
    for g in np.unique(groups[test]):
        in_g = test & (groups == g)
        start = t0[in_g].min()
        end = t1[in_g].max()
        overlap = (t0 <= end) & (t1 >= start)
        embargo = (t0 > end) & (t0 <= end + embargo_ms)
        keep &= ~(overlap | embargo)
    return keep


def purged_kfold(
    t0: np.ndarray, t1: np.ndarray, n_splits: int, embargo_ms: int
) -> list[tuple[np.ndarray, np.ndarray]]:
    groups = time_groups(t0, n_splits)
    folds = []
    for k in range(n_splits):
        test = groups == k
        folds.append(
            (np.flatnonzero(train_mask(t0, t1, test, groups, embargo_ms)), np.flatnonzero(test))
        )
    return folds


def cpcv(
    t0: np.ndarray,
    t1: np.ndarray,
    n_groups: int,
    n_test_groups: int,
    embargo_ms: int,
) -> tuple[list[tuple[np.ndarray, np.ndarray]], np.ndarray, list[tuple[int, ...]]]:
    """
    Combinatorial purged CV splits.

    Returns the (train, test) index splits, each event's group, and the test groups of each
    split. Every group appears in C(N-1, k-1) splits, which gives that many backtest paths.
    """
    groups = time_groups(t0, n_groups)
    splits = []
    combos = list(combinations(range(n_groups), n_test_groups))
    for combo in combos:
        test = np.isin(groups, combo)
        splits.append(
            (np.flatnonzero(train_mask(t0, t1, test, groups, embargo_ms)), np.flatnonzero(test))
        )
    return splits, groups, combos


def cpcv_paths(n_groups: int, combos: list[tuple[int, ...]]) -> np.ndarray:
    """
    Path assignment: paths[p, g] is the split index whose prediction of group g forms path p.
    """
    n_paths = sum(1 for c in combos if 0 in c)
    paths = np.full((n_paths, n_groups), -1, dtype=np.int64)
    used = np.zeros(n_groups, dtype=np.int64)
    for s, combo in enumerate(combos):
        for g in combo:
            paths[used[g], g] = s
            used[g] += 1
    return paths
