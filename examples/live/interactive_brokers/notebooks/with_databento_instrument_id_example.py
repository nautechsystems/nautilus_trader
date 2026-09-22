"""
Interactive Brokers with a Databento instrument ID.
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
# # Interactive Brokers with a Databento instrument ID
#
# This example requests an instrument by its Databento-style Nautilus ID, then uses the qualified IB
# contract for historical data, live subscriptions, and an optional bracket order. It builds
# offline by default.

# %%
from __future__ import annotations

import os
import sys
from collections.abc import Sequence
from pathlib import Path
from typing import Any


sys.path.insert(0, str(Path(__file__).resolve().parents[1]))

from _common import build_ib_live_node
from _common import default_ym_future_instrument_id
from _common import env_bool
from _common import env_int
from _common import instrument_provider_config
from _common import resolve_ib_endpoint
from _common import schedule_node_stop
from _common import set_databento_request_contracts_default
from ib_v2_order_strategies import IbV2OrderStrategy
from ib_v2_order_strategies import bar_type_from_env
from ib_v2_order_strategies import env_price
from ib_v2_order_strategies import env_quantity
from ib_v2_order_strategies import ib_client_id

from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model import Bar
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import OrderListId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import TimeInForce


# %%
INSTRUMENT_ID = os.getenv(
    "IB_V2_DATABENTO_INSTRUMENT_ID",
    default_ym_future_instrument_id(),
)


# %% [markdown]
# The instrument callback is the boundary between symbol resolution and data or order activity. The
# strategy requests 30 minutes of bars only after IB returns the qualified instrument.


# %%
class DatabentoInstrumentIdExample(IbV2OrderStrategy):
    """
    Databento instrument id example.
    """

    strategy_id_value = "IB-V2-DB-ID-STRATEGY"
    instrument_id_value = INSTRUMENT_ID

    def __init__(self) -> None:
        """
        Initialize the instance.
        """
        super().__init__()
        self.bar_type = bar_type_from_env(
            "IB_V2_DATABENTO_INSTRUMENT_BAR_TYPE",
            self.instrument_id,
        )
        self._seen_instrument_ids: set[str] = set()
        self._startup_requested = False
        self._live_trades_subscribed = False
        self._trade_count = 0
        self._bar_count = 0
        self._max_prints = env_int("IB_V2_SUBSCRIPTION_MAX_PRINTS", 5)

    def on_start(self) -> None:
        """
        On start.
        """
        print(f"{self.strategy_id}: requesting {self.instrument_id}", flush=True)
        self.request_instrument(self.instrument_id, client_id=ib_client_id())

        contracts = os.getenv("IB_V2_DATABENTO_REQUEST_CONTRACTS")
        if contracts:
            self.request_instruments(
                client_id=ib_client_id(),
                params={"ib_contracts": contracts},
            )

    def on_instrument(self, instrument: Any) -> None:
        """
        On instrument.
        """
        instrument_id = str(instrument.id)
        if instrument_id not in self._seen_instrument_ids:
            self._seen_instrument_ids.add(instrument_id)
            print(f"{self.strategy_id}: received instrument: {instrument.id}", flush=True)

        if instrument.id != self.instrument_id or self._startup_requested:
            return

        self.instrument = instrument
        self._startup_requested = True
        start_ns = self.clock.timestamp_ns() - (30 * 60 * 1_000_000_000)
        self.request_bars(
            self.bar_type,
            start=unix_nanos_to_dt(start_ns),
            client_id=ib_client_id(),
        )

        if env_bool("IB_V2_ENABLE_LIVE_TRADES"):
            self._live_trades_subscribed = True
            self.subscribe_trades(self.instrument_id, client_id=ib_client_id())

        if env_bool("IB_V2_ENABLE_LIVE_BARS"):
            self.subscribe_bars(
                self.bar_type,
                client_id=ib_client_id(),
                params={"start_ns": str(start_ns)},
            )

        if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION"):
            self.submit_example_orders()

    def submit_example_orders(self) -> None:
        """
        Submit example orders.
        """
        if self._orders_submitted:
            return

        self._orders_submitted = True
        quantity = env_quantity("IB_V2_DATABENTO_BRACKET_QUANTITY")
        entry_id = self.client_order_id("ENTRY")
        target_id = self.client_order_id("TARGET")
        stop_id = self.client_order_id("STOP")
        order_list_id = OrderListId.from_str(f"{self.strategy_id}-BRACKET")
        linked_ids = [entry_id, target_id, stop_id]
        entry = self.market_order(
            entry_id,
            OrderSide.BUY,
            quantity,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
        )
        target = self.limit_order(
            target_id,
            OrderSide.SELL,
            quantity,
            env_price("IB_V2_DATABENTO_BRACKET_TARGET_PRICE", "46755.00"),
            TimeInForce.GTC,
            contingency_type=ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )
        stop = self.stop_market_order(
            stop_id,
            OrderSide.SELL,
            quantity,
            env_price("IB_V2_DATABENTO_BRACKET_STOP_PRICE", "46735.00"),
            contingency_type=ContingencyType.OTO,
            order_list_id=order_list_id,
            linked_order_ids=linked_ids,
            parent_order_id=entry_id,
        )
        self.submit_ib_order(entry)
        self.submit_ib_order(target)
        self.submit_ib_order(stop)

    def on_trade(self, trade: Any) -> None:
        """
        On trade.
        """
        self._trade_count += 1
        if self._trade_count <= self._max_prints:
            print(f"{self.strategy_id}: trade #{self._trade_count}: {trade}", flush=True)

    def on_bar(self, bar: Any) -> None:
        """
        On bar.
        """
        self._bar_count += 1
        if self._bar_count <= self._max_prints:
            print(f"{self.strategy_id}: bar #{self._bar_count}: {bar}", flush=True)

    def on_historical_bars(self, bars: Sequence[Bar]) -> None:
        """
        On historical bars.
        """
        print(f"{self.strategy_id}: received {len(bars)} historical bar(s)", flush=True)

    def on_position_opened(self, event: Any) -> None:
        """
        On position opened.
        """
        print(f"{self.strategy_id}: position opened: {event}", flush=True)

    def on_stop(self) -> None:
        """
        On stop.
        """
        if self._live_trades_subscribed:
            self.unsubscribe_trades(self.instrument_id, client_id=ib_client_id())
        if self._startup_requested and env_bool("IB_V2_ENABLE_LIVE_BARS"):
            self.unsubscribe_bars(self.bar_type, client_id=ib_client_id())
        super().on_stop()


# %%
def main() -> None:
    """
    Run the example.
    """
    set_databento_request_contracts_default()
    host, port = resolve_ib_endpoint()
    account_id = os.getenv("TWS_ACCOUNT") if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") else None
    if env_bool("IB_V2_ENABLE_ORDER_SUBMISSION") and account_id is None:
        raise RuntimeError("Set TWS_ACCOUNT before enabling bracket order submission")

    provider_config = instrument_provider_config(load_ids=[INSTRUMENT_ID])
    node = build_ib_live_node(
        name="IB-V2-DB-ID-001",
        trader_id="IB-V2-DB-ID-001",
        host=host,
        port=port,
        data_client_id=env_int("IB_V2_DATA_CLIENT_ID", 2),
        exec_client_id=env_int("IB_V2_EXEC_CLIENT_ID", 3),
        account_id=account_id,
        provider_config=provider_config,
    )
    node.add_strategy(DatabentoInstrumentIdExample())

    print(f"Built Databento-ID node for {INSTRUMENT_ID}.", flush=True)
    if env_bool("IB_V2_RUN_NODE"):
        schedule_node_stop(node, env_int("IB_V2_AUTO_STOP_SECONDS", 30))
        node.run()


# %%
if __name__ == "__main__":
    main()
