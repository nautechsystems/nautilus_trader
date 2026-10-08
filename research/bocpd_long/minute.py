"""
Sealed loader for the 1-minute kline archive.

Monthly files are selected by name before anything is read: files for months at or after
the seal month are never opened. Rows are then filtered on `close_time` as a second guard.
Mirrored symbols invert prices (high and low swap).
"""

from __future__ import annotations

import re
from datetime import UTC
from datetime import datetime
from functools import lru_cache
from pathlib import Path

import polars as pl

from bocpd_long import bars
from bocpd_long import config


KLINES_DIR = config.ROOT / "binance_data" / "klines_1m"
_MONTH_RE = re.compile(r"-1m-(\d{4})-(\d{2})\.parquet$")
_SEAL = datetime.fromtimestamp(config.SEAL_START_MS / 1000, UTC)
SEAL_MONTH = (_SEAL.year, _SEAL.month)


def unsealed_files(symbol: str) -> list[Path]:
    out = []
    for path in sorted((KLINES_DIR / symbol).glob("*.parquet")):
        m = _MONTH_RE.search(path.name)
        if m is None:
            raise ValueError(f"Unexpected file name: {path}")
        if (int(m[1]), int(m[2])) < SEAL_MONTH:
            out.append(path)
    return out


@lru_cache(maxsize=4)
def load_minutes(symbol: str) -> pl.DataFrame:
    """
    1-minute bars (open_time, close_time, high, low, close) before the seal, sorted.
    """
    real = symbol.removesuffix(config.MIRROR_SUFFIX)
    df = (
        pl.read_parquet(
            unsealed_files(real), columns=["open_time", "close_time", "high", "low", "close"]
        )
        .filter(pl.col("close_time") < config.SEAL_START_MS)
        .with_columns(pl.col("high", "low", "close").cast(pl.Float64))
        .sort("open_time")
        .unique("open_time", keep="first", maintain_order=True)
    )
    if bars.is_mirror(symbol):
        df = df.with_columns(
            (1.0 / pl.col("low")).alias("high"),
            (1.0 / pl.col("high")).alias("low"),
            (1.0 / pl.col("close")).alias("close"),
        )
    return df
