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
    max_trade_size : Decimal
        The maximum order size per trade.
    trigger_min_size : PositiveFloat, default 100.0
        The minimum size on the larger side to trigger an order.
    trigger_imbalance_ratio : PositiveFloat, default 0.20
        The imbalance ratio threshold (smaller / larger). When the ratio
        falls below this value, a trigger fires. For example, with a ratio
        of 0.2 and bid size of 100, a buy triggers when ask size < 20.
    min_seconds_between_triggers : NonNegativeFloat, default 1.0
        The minimum time between triggers.
    book_type : str or BookType, default 'L2_MBP'
        The order book type for the strategy.
    use_quote_ticks : bool, default False
        If quote ticks should be used (requires L1_MBP book type).
    dry_run : bool, default False
        If dry run mode is active (no orders submitted).

    """

    instrument_id: InstrumentId
    max_trade_size: Decimal
    trigger_min_size: PositiveFloat = 100.0
    trigger_imbalance_ratio: PositiveFloat = 0.20
    min_seconds_between_triggers: NonNegativeFloat = 1.0
    book_type: str | BookType = "L2_MBP"
    use_quote_ticks: bool = False
    dry_run: bool = False


class OrderBookImbalance(Strategy):
    """
    A strategy that sends FOK limit orders when there is a bid/ask size imbalance in the
    order book.

    When bid size significantly exceeds ask size (ratio below threshold),
    the strategy buys at the ask. When ask size exceeds bid size, it sells
    at the bid. Orders use fill-or-kill to avoid partial fills.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : OrderBookImbalanceConfig
        The configuration for the instance.

    """

    def __init__(self, config: OrderBookImbalanceConfig) -> None:
        assert 0 < config.trigger_imbalance_ratio < 1
        super().__init__(config)

        self.instrument: Instrument | None = None
        self.book_type: BookType | None = None
        self._book: OrderBook | None = None
        self._max_qty: Quantity = Quantity.from_str(str(config.max_trade_size))
        self._last_trigger_timestamp: datetime.datetime | None = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        book_type = self.config.book_type
        if isinstance(book_type, str):
            book_type = book_type_from_str(book_type)

        if self.config.use_quote_ticks:
            self.book_type = BookType.L1_MBP
            self._book = OrderBook(self.instrument.id, self.book_type)
            self.subscribe_quote_ticks(self.instrument.id)
        else:
            self.book_type = book_type
            self.subscribe_order_book_deltas(self.instrument.id, self.book_type)

        self._last_trigger_timestamp = None

    def on_order_book_deltas(self, deltas: OrderBookDeltas) -> None:
        """
        Actions to be performed when order book deltas are received.
        """
        self.check_trigger()

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when a quote tick is received.
        """
        if self._book is not None:
            self._book.update_quote_tick(tick)
        self.check_trigger()

    def check_trigger(self) -> None:
        """
        Check for trigger conditions.
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        book = self._book or self.cache.order_book(self.config.instrument_id)
        if not book or not book.spread():
            return

        bid_size: Quantity | None = book.best_bid_size()
        ask_size: Quantity | None = book.best_ask_size()
        if not bid_size or bid_size <= 0 or not ask_size or ask_size <= 0:
            return

        smaller = min(bid_size, ask_size)
        larger = max(bid_size, ask_size)
        ratio = smaller / larger
        self.log.info(
            f"Book: [{bid_size}] {book.best_bid_price()} @ {book.best_ask_price()} [{ask_size}] ({ratio=:0.2f})",
        )

        if larger <= self.config.trigger_min_size:
            return
        if ratio >= self.config.trigger_imbalance_ratio:
            return
        if self._is_cooldown_active():
            return
        if self.cache.orders_inflight(strategy_id=self.id):
            return

        if bid_size > ask_size:
            side = OrderSide.BUY
            price = book.best_ask_price()
            level_size = ask_size
        else:
            side = OrderSide.SELL
            price = book.best_bid_price()
            level_size = bid_size

        self._last_trigger_timestamp = self.clock.utc_now()

        if self.config.dry_run:
            return

        trade_qty = min(level_size, self._max_qty)
        order = self.order_factory.limit(
            instrument_id=self.instrument.id,
            price=self.instrument.make_price(price),
            order_side=side,
            quantity=self.instrument.make_qty(trade_qty),
            post_only=False,
            time_in_force=TimeInForce.FOK,
        )
        self.log.info(f"Hitting! {order=}", color=LogColor.BLUE)
        self.submit_order(order)

    def _is_cooldown_active(self) -> bool:
        if self._last_trigger_timestamp is None:
            return False
        seconds_since = (self.clock.utc_now() - self._last_trigger_timestamp).total_seconds()
        return seconds_since < self.config.min_seconds_between_triggers

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        self._book = None
        self._last_trigger_timestamp = None

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        if self.instrument is None:
            return

        self.cancel_all_orders(self.instrument.id)
        self.close_all_positions(self.instrument.id)
