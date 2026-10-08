"""
Load dollar bars and build the mirrored market.

The mirror of a market inverts its price (log price is negated): open and close are
inverted, high and low are swapped and inverted, and taker buy and sell flow are swapped.
Bar boundaries, quote volume and n_minutes are unchanged. Mirrored symbols get the
`MIRROR_SUFFIX` and are processed exactly like real symbols.
"""

from __future__ import annotations

from collections.abc import Iterator

import polars as pl

from bocpd_long import config


def is_mirror(symbol: str) -> bool:
    return symbol.endswith(config.MIRROR_SUFFIX)


def real_symbols() -> list[str]:
    # Symbols whose first bar closes in the sealed period are excluded entirely
    return sorted(
        p.stem
        for p in config.BARS_DIR.glob("*.parquet")
        if pl.scan_parquet(p).select(pl.col("close_time").min()).collect().item()
        < config.SEAL_START_MS
    )


def load_bars(symbol: str) -> pl.DataFrame:
    df = (
        pl.read_parquet(config.BARS_DIR / f"{symbol}.parquet")
        .filter(pl.col("close_time") < config.SEAL_START_MS)
        .sort("close_time")
    )
    return df.select(
        "open_time",
        "close_time",
        "open",
        "high",
        "low",
        "close",
        "volume",
        "quote_volume",
        "taker_buy_volume",
        "taker_buy_quote_volume",
        pl.col("n_minutes").cast(pl.Int64),
    )


def mirror_bars(df: pl.DataFrame) -> pl.DataFrame:
    return df.with_columns(
        (1.0 / pl.col("open")).alias("open"),
        (1.0 / pl.col("low")).alias("high"),
        (1.0 / pl.col("high")).alias("low"),
        (1.0 / pl.col("close")).alias("close"),
        (pl.col("volume") - pl.col("taker_buy_volume")).alias("taker_buy_volume"),
        (pl.col("quote_volume") - pl.col("taker_buy_quote_volume")).alias("taker_buy_quote_volume"),
    )


def iter_panel() -> Iterator[tuple[str, pl.DataFrame]]:
    """
    Yield (symbol, bars) for every real symbol followed by its mirror.
    """
    for symbol in real_symbols():
        df = load_bars(symbol)
        yield symbol, df
        yield symbol + config.MIRROR_SUFFIX, mirror_bars(df)
