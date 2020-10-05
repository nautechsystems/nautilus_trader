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

from datetime import timedelta

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.strategy import TradingStrategy
from tests.test_kit.mocks import ObjectStorer


class PyStrategy(TradingStrategy):
    """
    A strategy which is empty and does nothing.
    """

    def __init__(self, bar_type: BarType):
        """Initialize a new instance of the PyStrategy class."""
        super().__init__(order_id_tag="001")

        self.bar_type = bar_type
        self.object_storer = ObjectStorer()

    def on_start(self):
        self.subscribe_bars(self.bar_type)

    def on_bar(self, bar_type, bar):
        self.object_storer.store_2(bar_type, bar)

    def on_event(self, event):
        self.object_storer.store(event)


class EmptyStrategy(TradingStrategy):
    """
    An empty strategy which does nothing.
    """

    def __init__(self, order_id_tag):
        """
        Initialize a new instance of the EmptyStrategy class.

        :param order_id_tag: The order_id tag for the strategy (should be unique at trader level).
        """
        super().__init__(order_id_tag=order_id_tag)


class TickTock(TradingStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """

    def __init__(self, instrument, bar_type):
        """Initialize a new instance of the TickTock class."""
        super().__init__(order_id_tag="000")

        self.instrument = instrument
        self.bar_type = bar_type
        self.store = []
        self.timer_running = False
        self.time_alert_counter = 0

    def on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.bar_type.symbol)

    def on_quote_tick(self, tick):
        self.log.info(f"Received Tick({tick})")
        self.store.append(tick)

    def on_bar(self, bar_type, bar):
        self.log.info(f"Received {bar_type} Bar({bar})")
        self.store.append(bar)
        if not self.timer_running:
            timer_name = "Test-Timer"
            self.clock.set_timer(name=timer_name, interval=timedelta(seconds=10))
            self.timer_running = True
            self.log.info(f"Started timer {timer_name}.")

        self.time_alert_counter += 1

        time_alert_name = f"Test-Alert-{self.time_alert_counter}"
        alert_time = bar.timestamp + timedelta(seconds=30)

        self.clock.set_time_alert(name=time_alert_name, alert_time=alert_time)
        self.log.info(f"Set time alert time_alert_name for {alert_time}.")

    def on_event(self, event):
        self.store.append(event)


class TestStrategy1(TradingStrategy):
    """
    A simple strategy for unit testing.
    """

    __test__ = False

    def __init__(self, bar_type, id_tag_strategy="001"):
        """Initialize a new instance of the TestStrategy1 class."""
        super().__init__(order_id_tag=id_tag_strategy)

        self.object_storer = ObjectStorer()
        self.bar_type = bar_type

        self.ema1 = ExponentialMovingAverage(10)
        self.ema2 = ExponentialMovingAverage(20)

        self.register_indicator_for_bars(self.bar_type, self.ema1)
        self.register_indicator_for_bars(self.bar_type, self.ema2)

        self.position_id = None

    def on_start(self):
        self.object_storer.store("custom start logic")

    def on_quote_tick(self, tick):
        self.object_storer.store(tick)

    def on_bar(self, bar_type, bar):
        self.object_storer.store((bar_type, Bar))

        if bar_type.equals(self.bar_type):
            if self.ema1.value > self.ema2.value:
                buy_order = self.order_factory.market(
                    self.bar_type.symbol,
                    OrderSide.BUY,
                    100000,
                )

                self.submit_order(buy_order)
                self.position_id = buy_order.cl_ord_id

            elif self.ema1.value < self.ema2.value:
                sell_order = self.order_factory.market(
                    self.bar_type.symbol,
                    OrderSide.SELL,
                    100000,
                )

                self.submit_order(sell_order)
                self.position_id = sell_order.cl_ord_id

    def on_instrument(self, instrument):
        self.object_storer.store(instrument)

    def on_event(self, event):
        self.object_storer.store(event)

    def on_stop(self):
        self.object_storer.store("custom stop logic")

    def on_reset(self):
        self.object_storer.store("custom reset logic")

    def on_save(self):
        self.object_storer.store("custom save logic")
        return {}

    def on_load(self, state):
        self.object_storer.store("custom load logic")

    def on_dispose(self):
        self.object_storer.store("custom dispose logic")


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
            extra_id_tag: str='',
    ):
        """
        Initialize a new instance of the EMACross class.

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
        extra_id_tag : str, optional
            An additional order identifier tag.

        """
        super().__init__(order_id_tag=symbol.code.replace('/', '') + extra_id_tag)

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
                          f"[{self.bar_count(self.bar_type)}]...")
            return  # Wait for indicators to warm up...

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.execution.is_flat(self.symbol, self.id):
                self.buy(1000000)
            elif self.execution.is_net_long(self.symbol, self.id):
                pass
            else:
                positions = self.execution.positions_open()
                if len(positions) > 0:
                    self.flatten_position(positions[0])
                    self.buy(1000000)

        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.execution.is_flat(self.symbol, self.id):
                self.sell(1000000)
            elif self.execution.is_net_short(self.symbol, self.id):
                pass
            else:
                positions = self.execution.positions_open()
                if len(positions) > 0:
                    self.flatten_position(positions[0])
                    self.sell(1000000)

    def buy(self, quantity: int):
        """
        Users simple buy method (example).
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
        self.unsubscribe_instrument(self.symbol)
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.symbol)
