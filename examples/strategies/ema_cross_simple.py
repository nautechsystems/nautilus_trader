# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import PositionEvent
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.trading.strategy import TradingStrategy


class EMACross(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position in that
    direction.
    """

    def __init__(
            self,
            symbol: Symbol,
            bar_spec: BarSpecification,
            fast_ema: int=10,
            slow_ema: int=20,
    ):
        """
        Initialize a new instance of the EMACross class.

        :param symbol: The symbol for the strategy.
        :param bar_spec: The bar specification for the strategy.
        :param fast_ema: The fast EMA period.
        :param slow_ema: The slow EMA period.
        """
        super().__init__(order_id_tag=symbol.code.replace('/', ''))

        # Custom strategy variables
        self.symbol = symbol
        self.bar_type = BarType(symbol, bar_spec)

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema)
        self.slow_ema = ExponentialMovingAverage(slow_ema)

    def on_start(self):
        """Actions to be performed on strategy start."""
        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)

    def on_quote_tick(self, tick: QuoteTick):
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        :param tick: The quote tick received.
        """
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        :param bar_type: The bar type received.
        :param bar: The bar received.
        """
        self.log.info(f"Received {bar_type} Bar({bar})")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(f"Waiting for indicators to warm up "
                          f"[{self.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.execution.is_flat(self.symbol, self.id):
                self.buy(Quantity(1000000))
            elif self.execution.is_net_long(self.symbol, self.id):
                pass
            else:
                self.buy(Quantity(2000000))

        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.execution.is_flat(self.symbol, self.id):
                self.sell(Quantity(1000000))
            elif self.execution.is_net_short(self.symbol, self.id):
                pass
            else:
                self.sell(Quantity(2000000))

    def buy(self, quantity):
        """
        Users simple buy method (example).
        """
        order = self.order_factory.market(
            symbol=self.symbol,
            order_side=OrderSide.BUY,
            quantity=quantity,
        )

        self.submit_order(order)

    def sell(self, quantity):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.market(
            symbol=self.symbol,
            order_side=OrderSide.SELL,
            quantity=quantity,
        )

        self.submit_order(order)

    def on_data(self, data):
        """
        Actions to be performed when the strategy is running and receives a data object.

        :param data: The data object received.
        """
        pass

    def on_event(self, event):
        """
        Actions to be performed when the strategy is running and receives an event.

        :param event: The event received.
        """
        if isinstance(event, PositionEvent):
            self.net_position = event.position.side

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        pass

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        pass

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Note: "OrderIdCount' and 'PositionIdCount' are reserved keys for
        the returned state dictionary.
        """
        return {}

    def on_load(self, state: {}):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.
        """
        pass

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.
        """
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.symbol)
