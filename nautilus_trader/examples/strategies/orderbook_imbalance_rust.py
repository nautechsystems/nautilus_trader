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

import datetime
from decimal import Decimal

from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.rust.common import LogColor
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
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
    max_trade_size : Decimal
        The max position size per trade (size on the level can be less).
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
        If quotes should be used.

    """

    instrument_id: InstrumentId
    max_trade_size: Decimal
    trigger_min_size: PositiveFloat = 100.0
    trigger_imbalance_ratio: PositiveFloat = 0.20
    min_seconds_between_triggers: NonNegativeFloat = 1.0
    book_type: str = "L2_MBP"
    use_quote_ticks: bool = False


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

        self.instrument: Instrument | None = None
        if self.config.use_quote_ticks:
            assert self.config.book_type == "L1_MBP"
        self.book_type: nautilus_pyo3.BookType = nautilus_pyo3.BookType(self.config.book_type)
        self._last_trigger_timestamp: datetime.datetime | None = None

        # We need to initialize the Rust pyo3 objects
        pyo3_instrument_id = nautilus_pyo3.InstrumentId.from_str(self.config.instrument_id.value)
        self.book = nautilus_pyo3.OrderBook(pyo3_instrument_id, self.book_type)
        self.imbalance = nautilus_pyo3.BookImbalanceRatio()

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        if self.config.use_quote_ticks:
            self.book_type = nautilus_pyo3.BookType.L1_MBP
            self.subscribe_quote_ticks(self.instrument.id)
        else:
            self.book_type = book_type_from_str(self.config.book_type)
            self.subscribe_order_book_deltas(
                self.instrument.id,
                self.book_type,
                managed=False,  # <-- Manually applying deltas to book
                pyo3_conversion=True,  # <--- Will automatically convert to pyo3 objects
            )

        self._last_trigger_timestamp = self.clock.utc_now()

    def on_order_book_deltas(self, pyo3_deltas: nautilus_pyo3.OrderBookDeltas) -> None:
        """
        Actions to be performed when order book deltas are received.
        """
        self.book.apply_deltas(pyo3_deltas)
        self.imbalance.handle_book(self.book)
        self.check_trigger()

    def on_quote_tick(self, quote: QuoteTick) -> None:
        """
        Actions to be performed when a quote tick is received.
        """
        if self.config.use_quote_ticks:
            nautilus_pyo3.update_book_with_quote_tick(self.book, quote)
            self.imbalance.handle_book(self.book)
            self.check_trigger()

    def on_order_book(self, book: OrderBook) -> None:
        """
        Actions to be performed when an order book update is received.
        """
        self.check_trigger()

    def check_trigger(self) -> None:
        """
        Check for trigger conditions.
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        # This could be more efficient: for demonstration
        bid_price = self.book.best_bid_price()
        ask_price = self.book.best_ask_price()
        bid_size = self.book.best_bid_size()
        ask_size = self.book.best_ask_size()
        if not bid_size or not ask_size:
            self.log.warning("No market yet")
            return

        larger = max(bid_size.as_double(), ask_size.as_double())
        ratio = self.imbalance.value
        self.log.info(
            f"Book: {self.book.best_bid_price()} @ {self.book.best_ask_price()} ({ratio=:0.2f})",
        )
        seconds_since_last_trigger = (
            self.clock.utc_now() - self._last_trigger_timestamp
        ).total_seconds()

        if larger > self.config.trigger_min_siz and ratio < self.config.trigger_imbalance_ratio:
            self.log.info(
                "Trigger conditions met, checking for existing orders and time since last order",
            )
            if len(self.cache.orders_inflight(strategy_id=self.id)) > 0:
                self.log.info("Already have orders in flight - skipping.")
            elif seconds_since_last_trigger < self.config.min_seconds_between_triggers:
                self.log.info("Time since last order < min_seconds_between_triggers - skipping")
            elif bid_size.as_double() > ask_size.as_double():
                order = self.order_factory.limit(
                    instrument_id=self.instrument.id,
                    price=self.instrument.make_price(ask_price),
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
                    price=self.instrument.make_price(bid_price),
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
