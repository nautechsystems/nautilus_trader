"""
LightGBM primary (side) and meta (act / pass) models with purged-CV early stopping.
"""

from __future__ import annotations

import lightgbm as lgb
import numpy as np
import polars as pl

from bocpd_long import config
from bocpd_long.features import FEATURES


def sample_weight(df: pl.DataFrame) -> np.ndarray:
    """
    Return-attribution weights (AFML 4.10) scaled to average one.
    """
    w = df["attribution"].to_numpy()
    return w / w.mean()


def params(bagging_fraction: float) -> dict:
    return config.LGBM_PARAMS | {"bagging_fraction": float(bagging_fraction)}


def cv_oof(
    x: np.ndarray,
    y: np.ndarray,
    w: np.ndarray,
    folds: list[tuple[np.ndarray, np.ndarray]],
    prm: dict,
) -> tuple[np.ndarray, int]:
    """
    Purged-CV early stopping on the mean fold log loss; returns out-of-fold probabilities and
    the chosen number of rounds.
    """
    data = lgb.Dataset(x, label=y, weight=w, free_raw_data=False)
    res = lgb.cv(
        prm | {"metric": "binary_logloss"},
        data,
        num_boost_round=config.MAX_ROUNDS,
        folds=folds,
        callbacks=[lgb.early_stopping(config.EARLY_STOPPING, verbose=False)],
        return_cvbooster=True,
    )
    rounds = len(res["valid binary_logloss-mean"])
    oof = np.full(y.shape[0], np.nan)
    for booster, (_, test) in zip(res["cvbooster"].boosters, folds, strict=True):
        oof[test] = booster.predict(x[test], num_iteration=rounds)
    return oof, rounds


def fit(x: np.ndarray, y: np.ndarray, w: np.ndarray, prm: dict, rounds: int) -> lgb.Booster:
    return lgb.train(prm, lgb.Dataset(x, label=y, weight=w), num_boost_round=rounds)


def primary_xy(df: pl.DataFrame) -> tuple[np.ndarray, np.ndarray]:
    return df.select(FEATURES).to_numpy(), (df["bin"].to_numpy() == 1).astype(np.int64)


def meta_xy(df: pl.DataFrame, p_primary: np.ndarray) -> tuple[np.ndarray, np.ndarray, np.ndarray]:
    """
    Meta features, labels and the primary side. The label is 1 when the primary side
    matches the realized triple-barrier return.
    """
    side = np.where(p_primary >= 0.5, 1, -1)
    x = np.column_stack([df.select(FEATURES).to_numpy(), p_primary, side])
    y = (side * df["ret"].to_numpy() > 0).astype(np.int64)
    return x, y, side
