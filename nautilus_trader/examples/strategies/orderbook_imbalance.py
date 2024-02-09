# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import datetime
from decimal import Decimal

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class OrderBookImbalanceConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``OrderBookImbalance`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    max_trade_size : str
        The max position size per trade (volume on the level can be less).
    trigger_min_size : PositiveFloat, default 100.0
        The minimum size on the larger side to trigger an order.
    trigger_imbalance_ratio : PositiveFloat, default 0.20
        The ratio of bid:ask volume required to trigger an order (smaller
        value / larger value) ie given a trigger_imbalance_ratio=0.2, and a
        bid volume of 100, we will send a buy order if the ask volume is <
        20).
    min_seconds_between_triggers : NonNegativeFloat, default 1.0
        The minimum time between triggers.
    book_type : str, default 'L2_MBP'
        The order book type for the strategy.
    use_quote_ticks : bool, default False
        If quote ticks should be used.
    subscribe_ticker : bool, default False
        If tickers should be subscribed to.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OmsType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).

    """

    instrument_id: InstrumentId
    max_trade_size: Decimal
    trigger_min_size: PositiveFloat = 100.0
    trigger_imbalance_ratio: PositiveFloat = 0.20
    min_seconds_between_triggers: NonNegativeFloat = 1.0
    book_type: str = "L2_MBP"
    use_quote_ticks: bool = False
    subscribe_ticker: bool = False


class OrderBookImbalance(Strategy):
    """
    A simple strategy that sends FOK limit orders when there is a bid/ask imbalance in
    the order book.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : OrderbookImbalanceConfig
        The configuration for the instance.

    """

    def __init__(self, config: OrderBookImbalanceConfig) -> None:
        assert 0 < config.trigger_imbalance_ratio < 1
        super().__init__(config)

        # Configuration
        self.instrument_id = config.instrument_id
        self.max_trade_size = config.max_trade_size
        self.trigger_min_size = config.trigger_min_size
        self.trigger_imbalance_ratio = config.trigger_imbalance_ratio
        self.min_seconds_between_triggers = config.min_seconds_between_triggers
        self._last_trigger_timestamp: datetime.datetime | None = None
        self.instrument: Instrument | None = None
        if self.config.use_quote_ticks:
            assert self.config.book_type == "L1_MBP"
        self.book_type: BookType = book_type_from_str(self.config.book_type)

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        if self.config.use_quote_ticks:
            self.book_type = BookType.L1_MBP
            self.subscribe_quote_ticks(self.instrument.id)
        else:
            self.book_type = book_type_from_str(self.config.book_type)
            self.subscribe_order_book_deltas(self.instrument.id, self.book_type)

        # TODO: Need to subscribe for custom data type
        # if self.config.subscribe_ticker:
        #     self.subscribe_ticker(self.instrument.id)

        self._last_trigger_timestamp = self.clock.utc_now()

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when order book deltas are received.
        """
        self.check_trigger()

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when a delta is received.
        """
        self.check_trigger()

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        self.check_trigger()

    def check_trigger(self) -> None:
        """
        Check for trigger conditions.
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        # Fetch book from the cache being maintained by the `DataEngine`
        book = self.cache.order_book(self.instrument_id)
        if not book:
            self.log.error("No book being maintained.")
            return

        if not book.spread():
            return

        bid_size: Quantity | None = book.best_bid_size()
        ask_size: Quantity | None = book.best_ask_size()
        if (bid_size is None or bid_size <= 0) or (ask_size is None or ask_size <= 0):
            self.log.warning("No market yet.")
            return

        smaller = min(bid_size, ask_size)
        larger = max(bid_size, ask_size)
        ratio = smaller / larger
        self.log.info(
            f"Book: {book.best_bid_price()} @ {book.best_ask_price()} ({ratio=:0.2f})",
        )
        seconds_since_last_trigger = (
            self.clock.utc_now() - self._last_trigger_timestamp
        ).total_seconds()

        if larger > self.trigger_min_size and ratio < self.trigger_imbalance_ratio:
            self.log.info(
                "Trigger conditions met, checking for existing orders and time since last order",
            )
            if len(self.cache.orders_inflight(strategy_id=self.id)) > 0:
                self.log.info("Already have orders in flight - skipping.")
            elif seconds_since_last_trigger < self.min_seconds_between_triggers:
                self.log.info("Time since last order < min_seconds_between_triggers - skipping.")
            elif bid_size > ask_size:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(book.best_ask_price()),
                    order_side=OrderSide.BUY,
                    quantity=self.instrument.make_qty(ask_size),
                    post_only=False,
                    time_in_force=TimeInForce.FOK,
                )
                self._last_trigger_timestamp = self.clock.utc_now()
                self.log.info(f"Hitting! {order=}", color=LogColor.BLUE)
                self.submit_order(order)

            else:
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(book.best_bid_price()),
                    order_side=OrderSide.SELL,
                    quantity=self.instrument.make_qty(bid_size),
                    post_only=False,
                    time_in_force=TimeInForce.FOK,
                )
                self._last_trigger_timestamp = self.clock.utc_now()
                self.log.info(f"Hitting! {order=}", color=LogColor.BLUE)
                self.submit_order(order)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.instrument is None:
            return

        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)
