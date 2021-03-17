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

from decimal import Decimal

from nautilus_trader.core.message import Event
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.bar import Bar
from nautilus_trader.model.bar import BarSpecification
from nautilus_trader.model.bar import BarType
from nautilus_trader.model.data import GenericData
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instrument import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.order.stop_market import StopMarketOrder
from nautilus_trader.model.order_book_old import OrderBook
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick
from nautilus_trader.trading.strategy import TradingStrategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class EMACrossWithTrailingStop(TradingStrategy):
    """
    A simple moving average cross example strategy with a stop-market entry and
    trailing stop.

    When the fast EMA crosses the slow EMA then submits a stop-market order one
    tick above the current bar for BUY, or one tick below the current bar
    for SELL.

    If the entry order is filled then a trailing stop at a specified ATR
    distance is submitted and managed.

    Cancels all orders and flattens all positions on stop.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_spec: BarSpecification,
        trade_size: Decimal,
        fast_ema_period: int,
        slow_ema_period: int,
        atr_period: int,
        trail_atr_multiple: float,
        order_id_tag: str,  # Must be unique at 'trader level'
    ):
        """
        Initialize a new instance of the `EMACrossWithTrailingStop` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument identifier for the strategy.
        bar_spec : BarSpecification
            The bar specification for the strategy.
        trade_size : Decimal
            The position size per trade.
        fast_ema_period : int
            The period for the fast EMA indicator.
        slow_ema_period : int
            The period for the slow EMA indicator.
        atr_period : int
            The period for the ATR indicator.
        trail_atr_multiple : float
            The ATR multiple for the trailing stop.
        order_id_tag : str
            The unique order identifier tag for the strategy. Must be unique
            amongst all running strategies for a particular trader identifier.

        """
        super().__init__(order_id_tag=order_id_tag)

        # Custom strategy variables
        self.instrument_id = instrument_id
        self.bar_type = BarType(instrument_id, bar_spec)
        self.trade_size = trade_size
        self.trail_atr_multiple = trail_atr_multiple
        self.instrument = None  # Initialize in on_start
        self.tick_size = None  # Initialize in on_start

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(slow_ema_period)
        self.atr = AverageTrueRange(atr_period)

        # Users order management variables
        self.entry = None
        self.trailing_stop = None

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.data.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        self.tick_size = self.instrument.tick_size

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)
        self.register_indicator_for_bars(self.bar_type, self.atr)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)

    def on_instrument(self, instrument: Instrument):
        """
        Actions to be performed when the strategy is running and receives an
        instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

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
        # self.log.info(f"Received {order_book}")  # For debugging (must add a subscription)

    def on_quote_tick(self, tick: QuoteTick):
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The quote tick received.

        """
        pass

    def on_trade_tick(self, tick: TradeTick):
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

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
        self.log.info(f"Received {bar_type} {repr(bar)}")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up "
                f"[{self.data.bar_count(self.bar_type)}]..."
            )
            return  # Wait for indicators to warm up...

        if self.portfolio.is_flat(self.instrument_id):
            if self.fast_ema.value >= self.slow_ema.value:
                self.entry_buy()
                self.trailing_stop_sell(bar)
            else:  # fast_ema.value < self.slow_ema.value
                self.entry_sell()
                self.trailing_stop_buy(bar)
        else:
            self.manage_trailing_stop(bar)

    def entry_buy(self):
        """
        Users simple buy entry method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(self.trade_size),
        )

        self.submit_order(order)

    def entry_sell(self):
        """
        Users simple sell entry method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=Quantity(self.trade_size),
        )

        self.submit_order(order)

    def trailing_stop_buy(self, last_bar: Bar):
        """
        Users simple trailing stop BUY for (SHORT positions).

        Parameters
        ----------
        last_bar : Bar
            The last bar received.

        """
        price: Decimal = last_bar.high + (self.atr.value * self.trail_atr_multiple)
        order: StopMarketOrder = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=Quantity(self.trade_size),
            price=Price(price),
            reduce_only=True,
        )

        self.trailing_stop = order
        self.submit_order(order)

    def trailing_stop_sell(self, last_bar: Bar):
        """
        Users simple trailing stop SELL for (LONG positions).
        """
        price: Decimal = last_bar.low - (self.atr.value * self.trail_atr_multiple)
        order: StopMarketOrder = self.order_factory.stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=Quantity(self.trade_size),
            price=Price(price, self.instrument.price_precision),
            reduce_only=True,
        )

        self.trailing_stop = order
        self.submit_order(order)

    def manage_trailing_stop(self, last_bar: Bar):
        """
        Users simple trailing stop management method (example).

        Parameters
        ----------
        last_bar : Bar
            The last bar received.

        """
        if not self.trailing_stop:
            self.log.error("Trailing Stop order was None!")
            self.flatten_all_positions(self.instrument_id)
            return

        if self.trailing_stop.is_sell:
            new_trailing_price = last_bar.low - (
                self.atr.value * self.trail_atr_multiple
            )
            if new_trailing_price > self.trailing_stop.price:
                self.cancel_order(self.trailing_stop)
                self.trailing_stop_sell(last_bar)
        else:  # trailing_stop.is_buy
            new_trailing_price = last_bar.high + (
                self.atr.value * self.trail_atr_multiple
            )
            if new_trailing_price < self.trailing_stop.price:
                self.cancel_order(self.trailing_stop)
                self.trailing_stop_buy(last_bar)

    def on_data(self, data: GenericData):
        """
        Actions to be performed when the strategy is running and receives generic data.

        Parameters
        ----------
        data : GenericData
            The data received.

        """
        pass

    def on_event(self, event: Event):
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        if isinstance(event, OrderFilled):
            if event.cl_ord_id == self.trailing_stop.cl_ord_id:
                last_bar = self.data.bar(self.bar_type)
                if event.order_side == OrderSide.BUY:
                    self.trailing_stop_sell(last_bar)
                elif event.order_side == OrderSide.SELL:
                    self.trailing_stop_buy(last_bar)
            elif event.cl_ord_id == self.trailing_stop.cl_ord_id:
                self.trailing_stop = None

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)

    def on_reset(self):
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()
        self.atr.reset()

    def on_save(self) -> {}:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}

    def on_load(self, state: {}):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        pass

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        pass
