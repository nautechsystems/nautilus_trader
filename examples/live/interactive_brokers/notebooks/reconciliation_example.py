"""
Interactive Brokers reconciliation.
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
# # Interactive Brokers reconciliation
#
# This example enables startup reconciliation, loads a small stock set, and shows the optional data
# subscriptions used while inspecting the reconciled account. It builds offline by default. Set
# `IB_V2_RUN_NODE=1` and `TWS_ACCOUNT` to connect to a paper account.

# %%
from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import build_ib_live_node
from _common import default_aapl_instrument_id
from _common import default_stock_contracts
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop
from ib_v2_order_strategies import bar_type_from_env
from ib_v2_order_strategies import ib_client_id

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import StrategyId
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig


# %%
INSTRUMENT_ID = InstrumentId.from_str(
    os.getenv("IB_V2_SUBSCRIPTION_INSTRUMENT_ID", default_aapl_instrument_id()),
)
# Paper acceptance has also been verified with ESZ6.XCME while the stock market is closed.


# %%
class ReconciliationExample(Strategy):
    """
    Reconciliation example.
    """

    def __init__(self) -> None:
        """
        Initialize the instance.
        """
        super().__init__(
            StrategyConfig(
                strategy_id=StrategyId.from_str("IB-V2-RECONCILIATION-STRATEGY"),
            ),
        )
        self.instrument_id = INSTRUMENT_ID
        self.bar_type = bar_type_from_env(
            "IB_V2_SUBSCRIPTION_BAR_TYPE",
            self.instrument_id,
        )
        self._subscribed = False

    def on_start(self) -> None:
        """
        On start.
        """
        print(f"{self.strategy_id}: requesting {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

    def on_instrument(self, instrument: Any) -> None:
        """
        On instrument.
        """
        if instrument.id != self.instrument_id or self._subscribed:
            return

        self._subscribed = True
        if env_bool("IB_V2_SUBSCRIBE_QUOTES"):
            self.subscribe_quotes(self.instrument_id, client_id=ib_client_id())
        if env_bool("IB_V2_SUBSCRIBE_TRADES"):
            self.subscribe_trades(self.instrument_id, client_id=ib_client_id())
        if env_bool("IB_V2_SUBSCRIBE_BARS"):
            self.subscribe_bars(self.bar_type, client_id=ib_client_id())

    def on_quote(self, quote: Any) -> None:
        """
        On quote.
        """
        print(f"{self.strategy_id}: quote: {quote}", flush=True)

    def on_trade(self, trade: Any) -> None:
        """
        On trade.
        """
        print(f"{self.strategy_id}: trade: {trade}", flush=True)

    def on_bar(self, bar: Any) -> None:
        """
        On bar.
        """
        print(f"{self.strategy_id}: bar: {bar}", flush=True)


# %% [markdown]
# Reconciliation is enabled before the node is built. The execution client receives the IB account
# ID, while the provider config preloads the stock contracts used for the optional subscriptions.


# %%
def main() -> None:
    """
    Run the example.
    """
    account_id = os.getenv("TWS_ACCOUNT")
    run_node = env_bool("IB_V2_RUN_NODE")
    if run_node and account_id is None:
        raise RuntimeError("Set TWS_ACCOUNT to run the reconciliation example")

    os.environ.setdefault("IB_V2_RECONCILIATION", "1")
    host, port = resolve_ib_endpoint()
    provider_config = instrument_provider_config(
        load_ids=[str(INSTRUMENT_ID)],
        load_contracts=default_stock_contracts(),
    )
    node = build_ib_live_node(
        name="IB-V2-RECONCILIATION-001",
        trader_id="IB-V2-RECONCILIATION-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1501),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1502),
        account_id=account_id or "U1234567",
        provider_config=provider_config,
    )
    node.add_strategy(ReconciliationExample())

    print(f"Built reconciliation node for {INSTRUMENT_ID}.", flush=True)
    if run_node:
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
        node.run()


# %%
if __name__ == "__main__":
    main()
