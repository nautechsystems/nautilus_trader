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

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.trading.strategy import Strategy


class SimpleQuoterStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for the simple quoter strategy.
    """

    instrument_id: InstrumentId
    order_qty: Decimal = Decimal("1")
    tob_offset_ticks: int = 0
    log_data: bool = False


class SimpleQuoterStrategy(Strategy):
    """
    A quoter that places a limit order on each side of the book at a top-of-book offset.
    """

    def __init__(self, config: SimpleQuoterStrategyConfig) -> None:
        super().__init__(config)
        self.instrument = None
        self._tick_size = Decimal("0")
        self._price_offset = Decimal("0")
        self._order_qty = None
        self._bid_order = None
        self._ask_order = None

    def on_start(self) -> None:
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        self._tick_size = self.instrument.price_increment.as_decimal()
        offset_ticks = max(self.config.tob_offset_ticks, 0)
        self._price_offset = self._tick_size * offset_ticks
        self._order_qty = self.instrument.make_qty(self.config.order_qty)

        self.subscribe_quote_ticks(self.config.instrument_id)

    def on_quote_tick(self, quote: QuoteTick) -> None:
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        if self.config.log_data:
            self.log.info(repr(quote), LogColor.CYAN)

        # Check if closed
        if self._bid_order and self._bid_order.is_closed:
            self._bid_order = None
        if self._ask_order and self._ask_order.is_closed:
            self._ask_order = None

        bid_price = quote.bid_price.as_decimal() - self._price_offset
        ask_price = quote.ask_price.as_decimal() + self._price_offset

        if self._bid_order is None:
            price = self.instrument.make_price(bid_price)
            order = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.BUY,
                price=price,
                quantity=self._order_qty,
            )
            self._bid_order = order
            self.submit_order(order)

        if self._ask_order is None:
            price = self.instrument.make_price(ask_price)
            order = self.order_factory.limit(
                instrument_id=self.config.instrument_id,
                order_side=OrderSide.SELL,
                price=price,
                quantity=self._order_qty,
            )
            self._ask_order = order
            self.submit_order(order)

    def on_event(self, event) -> None:
        # Handle fills and reset state
        if isinstance(event, OrderFilled):
            if self._bid_order and event.client_order_id == self._bid_order.client_order_id:
                self._bid_order = None
            elif self._ask_order and event.client_order_id == self._ask_order.client_order_id:
                self._ask_order = None

    def on_stop(self) -> None:
        self.cancel_all_orders(self.config.instrument_id)
        self.close_all_positions(self.config.instrument_id)
        self._bid_order = None
        self._ask_order = None
