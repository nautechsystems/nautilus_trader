# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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
from decimal import Decimal

from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instrument import Instrument
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order_book_old import OrderBook
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.strategy import TradingStrategy


class TickTock(TradingStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """

    def __init__(self, instrument: Instrument, bar_type: BarType):
        """
        Initialize a new instance of the `TickTock` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the strategy.
        bar_type : BarType
            The bar type for the strategy.

        """
        super().__init__(order_id_tag="000")

        self.instrument = instrument
        self.bar_type = bar_type
        self.store = []
        self.timer_running = False
        self.time_alert_counter = 0

    def on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.bar_type.instrument_id)

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


class EMACross(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position in that
    direction.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_spec: BarSpecification,
        trade_size: Decimal,
        fast_ema: int = 10,
        slow_ema: int = 20,
        extra_id_tag: str = "",
    ):
        """
        Initialize a new instance of the `EMACross` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the strategy.
        bar_spec : BarSpecification
            The bar specification for the strategy.
        trade_size : Decimal
            The position size per trade.
        fast_ema : int
            The fast EMA period.
        slow_ema : int
            The slow EMA period.
        extra_id_tag : str
            An additional order identifier tag.

        """
        if extra_id_tag is None:
            extra_id_tag = ""
        super().__init__(
            order_id_tag=instrument_id.symbol.value.replace("/", "") + extra_id_tag
        )

        # Custom strategy variables
        self.instrument_id = instrument_id
        self.bar_type = BarType(instrument_id, bar_spec)
        self.trade_size = trade_size

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

    def on_order_book(self, order_book: OrderBook):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

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
            self.log.info(
                f"Waiting for indicators to warm up "
                f"[{self.data.bar_count(self.bar_type)}]..."
            )
            return  # Wait for indicators to warm up...

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.buy()
            elif self.portfolio.is_net_short(self.instrument_id):
                self.flatten_all_positions(self.instrument_id)
                self.buy()

        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.sell()
            elif self.portfolio.is_net_long(self.instrument_id):
                self.flatten_all_positions(self.instrument_id)
                self.sell()

    def buy(self):
        """
        Users simple buy method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(self.trade_size),
        )

        self.submit_order(order)

    def sell(self):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=Quantity(self.trade_size),
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
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.

        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {"example": b"123456"}

    def on_load(self, state: {}):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        self.log.info(f"Loaded users state {state['example']}")

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        self.unsubscribe_bars(self.bar_type)
