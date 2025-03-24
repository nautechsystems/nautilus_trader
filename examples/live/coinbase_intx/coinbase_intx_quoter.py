#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

import pandas as pd

from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxDataClientConfig
from nautilus_trader.adapters.coinbase_intx.config import CoinbaseIntxExecClientConfig
from nautilus_trader.adapters.coinbase_intx.constants import COINBASE_INTX
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveDataClientFactory
from nautilus_trader.adapters.coinbase_intx.factories import CoinbaseIntxLiveExecClientFactory
from nautilus_trader.cache.config import CacheConfig
from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import LiveExecEngineConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import StrategyConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.core.data import Data
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders import LimitOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS INTEGRATION IS STILL UNDER CONSTRUCTION. ***
# *** CONSIDER IT TO BE IN AN UNSTABLE BETA PHASE AND EXERCISE CAUTION. ***

instrument_id = InstrumentId.from_str("BTC-PERP.COINBASE_INTX")
offset_ticks = 10  # Number of ticks to offset limit orders from the market
trade_size = Decimal("0.01")
dry_run = False  # Set this to True to prevent trading

# Configure the trading node
config_node = TradingNodeConfig(
    trader_id=TraderId("TESTER-001"),
    logging=LoggingConfig(log_level="INFO", use_pyo3=True),
    exec_engine=LiveExecEngineConfig(
        reconciliation=True,
        # snapshot_orders=True,
        # snapshot_positions=True,
        # snapshot_positions_interval_secs=5.0,
    ),
    cache=CacheConfig(
        # database=DatabaseConfig(),
        encoding="msgpack",
        timestamps_as_iso8601=True,
        buffer_interval_ms=100,
    ),
    # message_bus=MessageBusConfig(
    #     database=DatabaseConfig(),
    #     encoding="json",
    #     timestamps_as_iso8601=True,
    #     buffer_interval_ms=100,
    #     streams_prefix="quoters",
    #     use_instance_id=False,
    #     # types_filter=[QuoteTick],
    #     autotrim_mins=30,
    # ),
    # heartbeat_interval=1.0,
    data_clients={
        COINBASE_INTX: CoinbaseIntxDataClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    exec_clients={
        COINBASE_INTX: CoinbaseIntxExecClientConfig(
            instrument_provider=InstrumentProviderConfig(load_all=True),
        ),
    },
    timeout_connection=60.0,
    timeout_reconciliation=20.0,
    timeout_portfolio=10.0,
    timeout_disconnection=5.0,
    timeout_post_stop=5.0,
)

# Instantiate the node with a configuration
node = TradingNode(config=config_node)


class TOBQuoterConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``TOBQuoter`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    trade_size : Decimal
        The position size per trade.
    dry_run : bool
        If the strategy should run without issuing order commands.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.

    """

    instrument_id: InstrumentId
    offset_ticks: int
    trade_size: Decimal
    dry_run: bool = False


class TOBQuoter(Strategy):
    """
    A simple market maker which joins the current market best bid and ask.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : TOBQuoterConfig
        The configuration for the instance.

    """

    def __init__(self, config: TOBQuoterConfig) -> None:
        super().__init__(config)

        self.instrument: Instrument | None = None  # Initialized in on_start

        # Users order management variables
        self.buy_order: LimitOrder | None = None
        self.sell_order: LimitOrder | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        # Subscribe to live data
        self.subscribe_quote_ticks(self.config.instrument_id)
        self.subscribe_trade_ticks(self.config.instrument_id)

        # self.subscribe_order_book_deltas(self.config.instrument_id)
        # self.subscribe_order_book_at_interval(
        #     self.config.instrument_id,
        #     depth=20,
        #     interval_ms=1000,
        # )  # For debugging

    def on_data(self, data: Data) -> None:
        """
        Actions to be performed when the strategy is running and receives data.

        Parameters
        ----------
        data : Data
            The data received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(data), LogColor.CYAN)

    def on_instrument(self, instrument: Instrument) -> None:
        """
        Actions to be performed when the strategy is running and receives an instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(instrument), LogColor.CYAN)

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(order_book), LogColor.CYAN)

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when the strategy is running and receives order book
        deltas.

        Parameters
        ----------
        deltas : OrderBookDeltas
            The order book deltas received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(deltas), LogColor.CYAN)

        book = self.cache.order_book(deltas.instrument_id)

        self.maintain_orders(book.best_bid_price(), book.best_ask_price())

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(tick), LogColor.CYAN)

        self.maintain_orders(tick.bid_price, tick.ask_price)

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        # For debugging (must add a subscription)
        self.log.info(repr(tick), LogColor.CYAN)

    def maintain_orders(self, best_bid: Price, best_ask: Price) -> None:
        if self.config.dry_run:
            return

        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        if self.buy_order and (self.buy_order.is_emulated or self.buy_order.is_open):
            # TODO: Optionally cancel-replace
            # self.cancel_order(self.buy_order)
            pass

        if not self.buy_order or self.buy_order.is_closed:
            quantity = self.instrument.make_qty(self.config.trade_size)
            offset = self.instrument.price_increment * self.config.offset_ticks
            price = self.instrument.make_price(best_bid - offset)
            self.create_buy_order(quantity, price)

        # Maintain sell orders
        if self.sell_order and (self.sell_order.is_emulated or self.sell_order.is_open):
            # TODO: Optionally cancel-replace
            # self.cancel_order(self.sell_order)
            pass

        if not self.sell_order or self.sell_order.is_closed:
            quantity = self.instrument.make_qty(self.config.trade_size)
            offset = self.instrument.price_increment * self.config.offset_ticks
            price = self.instrument.make_price(best_ask + offset)
            self.create_sell_order(quantity, price)

    def create_buy_order(self, quantity: Quantity, price: Price) -> None:
        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=quantity,
            price=price,
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=1),
            post_only=True,
        )

        self.buy_order = order
        self.submit_order(order)

    def create_sell_order(self, quantity: Quantity, price: Price) -> None:
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        order: LimitOrder = self.order_factory.limit(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=quantity,
            price=price,
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + pd.Timedelta(minutes=1),
            post_only=True,
        )

        self.sell_order = order
        self.submit_order(order)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.config.dry_run:
            return

        # Tests individual cancels:
        # for order in self.cache.orders_open(instrument_id=self.config.instrument_id):
        #     self.cancel_order(order)

        self.cancel_all_orders(self.config.instrument_id)
        self.close_all_positions(
            instrument_id=self.config.instrument_id,
            time_in_force=TimeInForce.IOC,  # <-- Required for Coinbase Intx
            reduce_only=False,
        )

        # Tests unsubscribe:
        self.unsubscribe_quote_ticks(self.config.instrument_id)
        self.unsubscribe_trade_ticks(self.config.instrument_id)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        self.atr.reset()


# Configure your strategy
strat_config = TOBQuoterConfig(
    use_uuid_client_order_ids=True,  # <-- Necessary for Coinbase Intx
    instrument_id=instrument_id,
    external_order_claims=[instrument_id],
    offset_ticks=offset_ticks,
    trade_size=trade_size,
    dry_run=dry_run,
)

# Instantiate your strategy
strategy = TOBQuoter(config=strat_config)

# Add your strategies and modules
node.trader.add_strategy(strategy)

# Register your client factories with the node (can take user-defined factories)
node.add_data_client_factory(COINBASE_INTX, CoinbaseIntxLiveDataClientFactory)
node.add_exec_client_factory(COINBASE_INTX, CoinbaseIntxLiveExecClientFactory)
node.build()


# Stop and dispose of the node with SIGINT/CTRL+C
if __name__ == "__main__":
    try:
        node.run()
    finally:
        node.dispose()
