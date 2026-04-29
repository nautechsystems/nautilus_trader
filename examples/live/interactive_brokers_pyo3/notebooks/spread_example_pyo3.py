#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import os
import threading
import time

from nautilus_trader.adapters.interactive_brokers.common import IB
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersInstrumentProviderConfig,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveDataClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3 import (
    InteractiveBrokersV1LiveExecClientFactory,
)
from nautilus_trader.adapters.interactive_brokers_pyo3.config import MarketDataType
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import RoutingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.examples.interactive_brokers import resolve_ib_endpoint
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.events import OrderAccepted
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderRejected
from nautilus_trader.model.events import OrderSubmitted
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import generic_spread_id_to_list
from nautilus_trader.model.identifiers import new_generic_spread_id
from nautilus_trader.trading.config import StrategyConfig
from nautilus_trader.trading.strategy import Strategy


IB_HOST, IB_PORT = resolve_ib_endpoint("IB_PYO3_HOST", "IB_PYO3_PORT")


class SpreadTestConfig(StrategyConfig, frozen=True):
    spread_instrument_id: InstrumentId


class SpreadTestStrategy(Strategy):
    def __init__(self, config: SpreadTestConfig):
        super().__init__(config=config)
        self.order_placed = False
        self.exit_order_placed = False
        self.entry_fill_received = False
        self.waiting_for_cleanup = False
        self.related_leg_ids: list[InstrumentId] = []
        self.spread_instrument = None
        self.execution_events: list[OrderFilled] = []
        self.order_events: list[OrderAccepted | OrderSubmitted | OrderRejected] = []
        self.quote_tick_count = 0
        self.instrument_loaded = False

    def on_start(self):
        self.log.info("=" * 80, color=LogColor.BLUE)
        self.log.info("SPREAD INSTRUMENT - DYNAMIC LOADING (PYO3)", color=LogColor.BLUE)
        self.log.info("=" * 80, color=LogColor.BLUE)
        self.log.info("Requesting spread instrument dynamically...")
        self.request_instrument(self.config.spread_instrument_id)

    def on_instrument(self, instrument):
        self.log.info(f"Received instrument: {instrument.id}")
        self.log.info(f"Instrument type: {type(instrument)}")
        self.spread_instrument = instrument
        self.related_leg_ids = [
            leg_id for leg_id, _ratio in generic_spread_id_to_list(instrument.id)
        ]
        self.instrument_loaded = True
        self.log.info("Subscribing to quote ticks for spread instrument...")
        self.subscribe_quote_ticks(instrument.id)
        self._cleanup_positions_then_maybe_enter()

    def _place_ratio_spread_order(self, instrument):
        self.log.info("=" * 60, color=LogColor.GREEN)
        self.log.info("PLACING SPREAD MARKET ORDER (DAY)", color=LogColor.GREEN)
        self.log.info("=" * 60, color=LogColor.GREEN)

        order = self.order_factory.market(
            instrument_id=self.config.spread_instrument_id,
            order_side=OrderSide.BUY,
            quantity=instrument.make_qty(3),
            time_in_force=TimeInForce.DAY,
        )

        self.log.info("Order details:")
        self.log.info(f"   Client Order ID: {order.client_order_id}")
        self.log.info(f"   Instrument: {order.instrument_id}")
        self.log.info(f"   Side: {order.side}")
        self.log.info(f"   Quantity: {order.quantity}")
        self.log.info(f"   Order Type: {order.order_type}")

        self.submit_order(order)
        self.order_placed = True
        self.log.info("Market order submitted for option spread", color=LogColor.GREEN)

    def _place_flatten_order(self, quantity):
        self.log.info("=" * 60, color=LogColor.YELLOW)
        self.log.info("PLACING FLATTENING SPREAD MARKET ORDER (DAY)", color=LogColor.YELLOW)
        self.log.info("=" * 60, color=LogColor.YELLOW)

        order = self.order_factory.market(
            instrument_id=self.config.spread_instrument_id,
            order_side=OrderSide.SELL,
            quantity=quantity,
            time_in_force=TimeInForce.DAY,
        )

        self.submit_order(order)
        self.exit_order_placed = True
        self.log.info("Flattening market order submitted for spread", color=LogColor.YELLOW)

    def _related_open_positions(self):
        positions = []
        for leg_id in self.related_leg_ids:
            positions.extend(self.cache.positions_open(instrument_id=leg_id))
        return positions

    def _cleanup_positions_then_maybe_enter(self):
        if self.is_exiting():
            return
        if self.order_placed or self.spread_instrument is None:
            return

        open_positions = self._related_open_positions()
        if open_positions:
            self.waiting_for_cleanup = True
            self.log.info(
                f"Found {len(open_positions)} open leg position(s); flattening before entry...",
                color=LogColor.YELLOW,
            )

            for position in open_positions:
                self.log.info(f"   Closing existing position: {position}")
                self.close_position(
                    position,
                    time_in_force=TimeInForce.DAY,
                    reduce_only=False,
                )
            return

        if self.waiting_for_cleanup:
            self.log.info("Existing leg positions flattened, continuing with entry order")
            self.waiting_for_cleanup = False

        self._place_ratio_spread_order(self.spread_instrument)

    def on_quote_tick(self, tick):
        self.quote_tick_count += 1
        if self.quote_tick_count <= 3:
            self.log.info(
                f"Quote tick #{self.quote_tick_count}: Bid={tick.bid_price}, Ask={tick.ask_price}",
                color=LogColor.CYAN,
            )

    def on_order_submitted(self, event: OrderSubmitted):
        self.order_events.append(("SUBMITTED", event))
        self.log.info(
            f"ORDER SUBMITTED: {event.client_order_id} | Account: {event.account_id}",
            color=LogColor.BLUE,
        )

    def on_order_accepted(self, event: OrderAccepted):
        self.order_events.append(("ACCEPTED", event))
        self.log.info(
            f"ORDER ACCEPTED: {event.client_order_id} | Venue Order ID: {event.venue_order_id}",
            color=LogColor.GREEN,
        )

    def on_order_rejected(self, event: OrderRejected):
        self.order_events.append(("REJECTED", event))
        self.log.error(f"ORDER REJECTED: {event.client_order_id} | Reason: {event.reason}")

    def on_order_filled(self, event: OrderFilled):
        self.execution_events.append(event)
        self.log.info("=" * 80, color=LogColor.MAGENTA)
        self.log.info(f"FILL #{len(self.execution_events)} RECEIVED", color=LogColor.MAGENTA)
        self.log.info("=" * 80, color=LogColor.MAGENTA)
        self.log.info(f"   Client Order ID: {event.client_order_id}")
        self.log.info(f"   Instrument: {event.instrument_id}")
        self.log.info(f"   Order Side: {event.order_side}")
        self.log.info(f"   Fill Quantity: {event.last_qty}")
        self.log.info(f"   Fill Price: {event.last_px}")
        self.log.info(f"   Commission: {event.commission}")
        self.log.info(f"   Trade ID: {event.trade_id}")
        self.log.info(f"   Info: {event.info}")
        if event.info and "avg_px" in event.info:
            self.log.info(f"   avg_px info: {event.info['avg_px']}", color=LogColor.GREEN)

        if (
            str(event.instrument_id) == str(self.config.spread_instrument_id)
            and not self.entry_fill_received
        ):
            self.entry_fill_received = True
            if not self.exit_order_placed:
                self._place_flatten_order(event.last_qty)

    def on_position_changed(self, event):
        if self.waiting_for_cleanup:
            self._cleanup_positions_then_maybe_enter()

    def on_position_closed(self, event):
        if self.waiting_for_cleanup:
            self._cleanup_positions_then_maybe_enter()

    def on_stop(self):
        self.log.info("=" * 80, color=LogColor.BLUE)
        self.log.info("FINAL TEST ANALYSIS", color=LogColor.BLUE)
        self.log.info("=" * 80, color=LogColor.BLUE)
        self.log.info(f"Instrument loaded dynamically: {'YES' if self.instrument_loaded else 'NO'}")
        self.log.info(f"Quote ticks received: {self.quote_tick_count}")
        self.log.info(f"Total fills received: {len(self.execution_events)}")
        self.log.info(f"Total order events: {len(self.order_events)}")


leg1_id = InstrumentId.from_str("ESM6 P6800.XCME")
leg2_id = InstrumentId.from_str("ESM6 P6775.XCME")
spread_id = new_generic_spread_id([(leg1_id, 1), (leg2_id, -1)])

instrument_provider = InteractiveBrokersInstrumentProviderConfig(
    build_futures_chain=False,
    build_options_chain=False,
    min_expiry_days=0,
    max_expiry_days=30,
)

config_node = TradingNodeConfig(
    trader_id="SPREAD-TEST",
    logging=LoggingConfig(log_level="INFO"),
    data_clients={
        IB: InteractiveBrokersDataClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=int(os.getenv("IB_PYO3_DATA_CLIENT_ID", "111")),
            instrument_provider=instrument_provider,
            market_data_type=MarketDataType.DelayedFrozen,
        ),
    },
    exec_clients={
        IB: InteractiveBrokersExecClientConfig(
            ibg_host=IB_HOST,
            ibg_port=IB_PORT,
            ibg_client_id=int(os.getenv("IB_PYO3_EXEC_CLIENT_ID", "112")),
            instrument_provider=instrument_provider,
            routing=RoutingConfig(default=True),
            account_id=os.environ.get("TWS_ACCOUNT"),
        ),
    },
    timeout_connection=90.0,
    timeout_reconciliation=5.0,
    timeout_portfolio=5.0,
    timeout_disconnection=5.0,
    timeout_post_stop=2.0,
)

node = TradingNode(config=config_node)
strategy = SpreadTestStrategy(
    config=SpreadTestConfig(
        spread_instrument_id=spread_id,
        manage_stop=True,
        market_exit_time_in_force=TimeInForce.DAY,
        market_exit_reduce_only=False,
    ),
)

node.trader.add_strategy(strategy)
node.add_data_client_factory(IB, InteractiveBrokersV1LiveDataClientFactory)
node.add_exec_client_factory(IB, InteractiveBrokersV1LiveExecClientFactory)
node.build()


def auto_stop_node(delay_seconds=20):
    def stop_after_delay():
        time.sleep(delay_seconds)
        node.stop()

    thread = threading.Thread(target=stop_after_delay, daemon=True)
    thread.start()


print(f"Testing spread: {spread_id}")
print("Order: 3 spread units")
print("Expected execution: Long 3 ESM6 P6800, Short 3 ESM6 P6775")
print("Using Interactive Brokers PyO3 factories")

if __name__ == "__main__":
    auto_stop_node(delay_seconds=20)
    try:
        node.run()
    except KeyboardInterrupt:
        node.stop()
    finally:
        node.dispose()
