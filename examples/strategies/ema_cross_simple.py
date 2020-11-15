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
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
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
        Initialize a new instance of the `EMACross` class.

        Parameters
        ----------
        symbol : Symbol
            The symbol for the strategy.
        bar_spec : BarSpecification
            The bar specification for the strategy.
        fast_ema : int
            The fast EMA period.
        slow_ema : int
            The slow EMA period.

        """
        # The order_id_tag should be unique at the 'trader level', here we are
        # just using the traded instruments symbol as the strategy order id tag.
        super().__init__(order_id_tag=symbol.code.replace('/', ""))

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

    def on_trade_tick(self, tick: TradeTick):
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """
        pass

    def on_quote_tick(self, tick: QuoteTick):
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick received.

        """
        pass

    def on_bar(self, bar_type: BarType, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar_type : BarType
            The bar type received.
        bar : Bar
            The bar received.

        """
        self.log.info(f"Received {bar_type} Bar({bar})")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(f"Waiting for indicators to warm up "
                          f"[{self.data.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.symbol):
                self.buy(1000000)
            elif self.portfolio.is_net_long(self.symbol):
                pass
            else:
                positions = self.execution.positions_open()
                if len(positions) > 0:
                    self.flatten_position(positions[0])
                    self.buy(1000000)

        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.symbol):
                self.sell(1000000)
            elif self.portfolio.is_net_short(self.symbol):
                pass
            else:
                positions = self.execution.positions_open()
                if len(positions) > 0:
                    self.flatten_position(positions[0])
                    self.sell(1000000)

    def buy(self, quantity: int):
        """
        Users simple buy method (example).

        Parameters
        ----------
        quantity : int
            The quantity for the sell order.

        """
        order = self.order_factory.market(
            symbol=self.symbol,
            order_side=OrderSide.BUY,
            quantity=Quantity(quantity),
        )

        self.submit_order(order)

    def sell(self, quantity: int):
        """
        Users simple sell method (example).

        Parameters
        ----------
        quantity : int
            The quantity for the sell order.

        """
        order = self.order_factory.market(
            symbol=self.symbol,
            order_side=OrderSide.SELL,
            quantity=Quantity(quantity),
        )

        self.submit_order(order)

    def on_data(self, data):
        """
        Actions to be performed when the strategy is running and receives a data object.

        Parameters
        ----------
        data : object
            The data object received.

        """
        pass

    def on_event(self, event):
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        pass

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.symbol)
        self.flatten_all_positions(self.symbol)

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        pass

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict

        Notes
        -----
        "OrderIdCount' is a reserved key for the returned state dictionary.

        """
        return {}

    def on_load(self, state: {}):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict
            The strategy state dictionary.

        """
        pass

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        self.unsubscribe_bars(self.bar_type)
