"""
Interactive Brokers market order.
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
# # Interactive Brokers market order
#
# This example selects a current ES futures contract, creates one market order, and configures the
# IB clients. It builds offline by default. Set `IB_V2_ENABLE_ORDER_SUBMISSION=1` and
# `IB_V2_RUN_NODE=1` only for a paper-account run.

# %%
from __future__ import annotations

import os
import sys
from pathlib import Path


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import build_ib_live_node
from _common import default_es_future_instrument_id
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop
from ib_v2_order_strategies import IbV2OrderStrategy
from ib_v2_order_strategies import env_order_side
from ib_v2_order_strategies import env_quantity

from nautilus_trader.model import OrderSide


# %%
INSTRUMENT_ID = os.getenv(
    "IB_V2_ORDER_INSTRUMENT_ID",
    default_es_future_instrument_id(),
)


# %% [markdown]
# The strategy waits until the selected instrument is available, then submits exactly one market
# order. Set `IB_V2_MARKET_SIDE=SELL` to reverse the default side.


# %%
class MarketOrderExample(IbV2OrderStrategy):
    """
    Market order example.
    """

    strategy_id_value = "IB-V2-MARKET-STRATEGY"
    instrument_id_value = INSTRUMENT_ID

    def submit_example_orders(self) -> None:
        """
        Submit example orders.
        """
        order = self.market_order(
            self.client_order_id("MARKET"),
            env_order_side("IB_V2_MARKET_SIDE", OrderSide.BUY),
            env_quantity("IB_V2_MARKET_QUANTITY"),
        )
        self.submit_ib_order(order)


# %%
def main() -> None:
    """
    Run the example.
    """
    host, port = resolve_ib_endpoint()
    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") else None
    if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") and account_id is None:
        raise RuntimeError("Set TWS_ACCOUNT before enabling market order submission")

    provider_config = instrument_provider_config(load_ids=[INSTRUMENT_ID])
    node = build_ib_live_node(
        name="IB-V2-MARKET-001",
        trader_id="IB-V2-MARKET-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 1511),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 1512),
        account_id=account_id,
        provider_config=provider_config,
    )
    node.add_strategy(MarketOrderExample())

    print(f"Built market-order node for {INSTRUMENT_ID}.", flush=True)
    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
        node.run()


# %%
if __name__ == "__main__":
    main()
