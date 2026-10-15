"""
Interactive Brokers option spread.
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
# # Interactive Brokers option spread
#
# This example builds a generic two-leg ES option spread and submits it as one market order. It
# builds offline by default. Set `IB_V2_ENABLE_ORDER_SUBMISSION=1` and `IB_V2_RUN_NODE=1` only for
# a paper-account run.

# %%
from __future__ import annotations

import os
import sys
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import build_ib_live_node
from _common import default_es_put_option_instrument_id
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop
from ib_v2_order_strategies import IbV2OrderStrategy
from ib_v2_order_strategies import env_quantity
from ib_v2_order_strategies import ib_order_tags

from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import new_generic_spread_id


# %% [markdown]
# `new_generic_spread_id` owns the generic spread grammar. A positive ratio buys the long leg and a
# negative ratio sells the short leg. The option IDs use the same relative contract selector as the
# other notebooks, so the default does not expire on a fixed date.

# %%
LONG_LEG_ID = InstrumentId.from_str(default_es_put_option_instrument_id(6800.0))
SHORT_LEG_ID = InstrumentId.from_str(default_es_put_option_instrument_id(6750.0))
DEFAULT_SPREAD_ID = new_generic_spread_id(
    [
        (LONG_LEG_ID, 1),
        (SHORT_LEG_ID, -1),
    ],
)
SPREAD_ID = InstrumentId.from_str(
    os.getenv("IB_V2_SPREAD_INSTRUMENT_ID", str(DEFAULT_SPREAD_ID)),
)


# %%
class SpreadOrderExample(IbV2OrderStrategy):
    """
    Spread order example.
    """

    strategy_id_value = "IB-V2-SPREAD-STRATEGY"
    instrument_id_value = str(SPREAD_ID)

    def __init__(self) -> None:
        """
        Initialize the instance.
        """
        super().__init__()
        self.instrument_id = SPREAD_ID
        self._flatten_submitted = False

    def submit_example_orders(self) -> None:
        """
        Submit example orders.
        """
        if not env_bool("IB_V2_SPREAD_SUBMIT", True):
            print(f"{self.strategy_id}: spread submission disabled", flush=True)
            return

        tags = (
            [ib_order_tags(non_guaranteed=True)]
            if env_bool("IB_V2_SPREAD_NON_GUARANTEED")
            else None
        )
        order = self.market_order(
            self.client_order_id("SPREAD"),
            OrderSide.BUY,
            env_quantity("IB_V2_SPREAD_QUANTITY"),
            tags=tags,
        )
        self.submit_ib_order(order)

    def on_order_submitted(self, event: Any) -> None:
        """
        On order submitted.
        """
        print(f"{self.strategy_id}: order submitted: {event}", flush=True)

    def on_order_rejected(self, event: Any) -> None:
        """
        On order rejected.
        """
        print(f"{self.strategy_id}: order rejected: {event}", flush=True)

    def on_order_filled(self, event: Any) -> None:
        """
        On order filled.
        """
        print(f"{self.strategy_id}: order filled: {event}", flush=True)
        if not env_bool("IB_V2_SPREAD_FLATTEN_ON_FILL") or self._flatten_submitted:
            return
        if event.instrument_id != self.instrument_id:
            return

        self._flatten_submitted = True
        flatten = self.market_order(
            self.client_order_id("SPREAD-FLATTEN"),
            OrderSide.SELL,
            event.last_qty,
            tags=(
                [ib_order_tags(non_guaranteed=True)]
                if env_bool("IB_V2_SPREAD_NON_GUARANTEED")
                else None
            ),
        )
        self.submit_ib_order(flatten)


# %%
def main() -> None:
    """
    Run the example.
    """
    host, port = resolve_ib_endpoint()
    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") else None
    if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") and account_id is None:
        raise RuntimeError("Set TWS_ACCOUNT before enabling spread order submission")

    provider_config = instrument_provider_config(load_ids=[str(SPREAD_ID)])
    node = build_ib_live_node(
        name="IB-V2-SPREAD-001",
        trader_id="IB-V2-SPREAD-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 111),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 112),
        account_id=account_id,
        provider_config=provider_config,
    )
    node.add_strategy(SpreadOrderExample())

    print(f"Built spread node for {SPREAD_ID}.", flush=True)
    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 20))
        node.run()


# %%
if __name__ == "__main__":
    main()
