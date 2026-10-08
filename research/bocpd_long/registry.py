"""
Trial registry: the daily Sharpe ratio of every strategy variant ever evaluated on the dev
period, so the deflated Sharpe ratio uses the full number of trials (49 when this strategy
was selected). Never shrink it: deleting trials would overstate the strategy's significance.
"""

from __future__ import annotations

import polars as pl

from bocpd_long import config


PATH = config.OUT_DIR / "trial_registry.csv"


def load() -> pl.DataFrame:
    if not PATH.exists():
        raise FileNotFoundError(f"trial registry missing: {PATH}")
    return pl.read_csv(PATH)


def register(name: str, sharpe_daily: float) -> None:
    reg = load()
    if name in reg["name"].to_list():
        reg = reg.with_columns(
            pl.when(pl.col("name") == name)
            .then(pl.lit(sharpe_daily))
            .otherwise(pl.col("sharpe_daily"))
            .alias("sharpe_daily"),
        )
    else:
        reg = pl.concat([reg, pl.DataFrame({"name": [name], "sharpe_daily": [sharpe_daily]})])
    reg.write_csv(PATH)


def sharpes() -> pl.Series:
    return load()["sharpe_daily"]
