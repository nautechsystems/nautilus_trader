"""
Interactive Brokers option chains.
"""

# ---
# jupyter:
#   jupytext:
#     formats: py:percent
#     text_representation:
#       extension: .py
#       format_name: percent
#       format_version: '1.3'
#   kernelspec:
#     display_name: Python 3 (ipykernel)
#     language: python
#     name: python3
# ---

# %% [markdown]
# # Interactive Brokers option chains

# %% [markdown]
# This example loads the option chains of a stock, two indexes, and a future through one instrument
# request, then prints how many options each expiry contains. It builds offline by default. Set
# `IB_V2_RUN_NODE=1` to connect and load the chains; it submits no orders.

# %%
from __future__ import annotations

import datetime as dt
import json
import sys
from collections import Counter
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import build_ib_live_node
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop
from ib_v2_order_strategies import ib_client_id

from nautilus_trader.model import StrategyId
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


# %% [markdown]
# Each contract spec sets `build_options_chain`, and `min_expiry_days` and `max_expiry_days` bound
# the expiries of the options to load. A stock or index underlying loads its options. A continuous
# future also sets `build_futures_chain`, which loads its futures contracts, and then loads the
# futures options of each one.

# %%
OPTION_CHAIN_CONTRACTS = [
    {
        "secType": "STK",
        "symbol": "SPY",
        "exchange": "SMART",
        "primaryExchange": "CBOE",
        "build_options_chain": True,
        "min_expiry_days": 0,
        "max_expiry_days": 3,
    },
    {
        "secType": "IND",
        "symbol": "SPX",
        "exchange": "CBOE",
        "build_options_chain": True,
        "min_expiry_days": 0,
        "max_expiry_days": 5,
    },
    {
        "secType": "CONTFUT",
        "exchange": "CME",
        "symbol": "ES",
        "build_futures_chain": True,
        "build_options_chain": True,
        "min_expiry_days": 0,
        "max_expiry_days": 2,
    },
    {
        "secType": "IND",
        "exchange": "EUREX",
        "symbol": "ESTX50",
        "build_options_chain": True,
        "min_expiry_days": 0,
        "max_expiry_days": 2,
    },
]


# %% [markdown]
# The strategy requests every chain at start. Each loaded instrument arrives through
# `on_instrument`, and the strategy counts options by instrument type, underlying, and expiry.


# %%
class OptionChainExample(Strategy):
    """
    Option chain example.
    """

    def __init__(self) -> None:
        """
        Initialize the instance.
        """
        super().__init__(
            StrategyConfig(strategy_id=StrategyId.from_str("IB-V2-OPTION-CHAIN-STRATEGY")),
        )
        self.expiries: Counter[tuple[str, str, str]] = Counter()

    def on_start(self) -> None:
        """
        On start.
        """
        print(f"{self.strategy_id}: requesting option chains", flush=True)
        self.request_instruments(
            client_id=ib_client_id(),
            params={"ib_contracts": json.dumps(OPTION_CHAIN_CONTRACTS)},
        )

    def on_instrument(self, instrument: Any) -> None:
        """
        On instrument.
        """
        expiration_ns = getattr(instrument, "expiration_ns", None)
        if expiration_ns is None or not hasattr(instrument, "strike_price"):
            return

        expiry = dt.datetime.fromtimestamp(expiration_ns / 1e9, tz=dt.UTC)
        key = (type(instrument).__name__, str(instrument.underlying), expiry.isoformat())
        self.expiries[key] += 1

    def on_stop(self) -> None:
        """
        On stop.
        """
        print(f"{self.strategy_id}: loaded {self.expiries.total()} options", flush=True)
        for (kind, underlying, expiry), count in sorted(self.expiries.items()):
            print(f"{kind} {underlying} expires {expiry}: {count}", flush=True)


# %%
def main() -> None:
    """
    Run the example.
    """
    host, port = resolve_ib_endpoint()
    node = build_ib_live_node(
        name="IB-V2-OPTION-CHAIN-001",
        trader_id="IB-V2-OPTION-CHAIN-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1531),
        provider_config=instrument_provider_config(),
    )
    node.add_strategy(OptionChainExample())

    print("Built option-chain node.", flush=True)
    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 60))
        node.run()


# %%
if __name__ == "__main__":
    main()
