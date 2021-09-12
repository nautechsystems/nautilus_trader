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
from typing import Any, Dict, Optional

from nautilus_trader.adapters.betfair.common import MAX_BET_PROB
from nautilus_trader.adapters.betfair.common import MIN_BET_PROB
from nautilus_trader.common.logging import LogColor
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.c_enums.instrument_status import InstrumentStatus
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.data.bar import Bar
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import QuoteTick
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.data.venue import InstrumentClosePrice
from nautilus_trader.model.data.venue import InstrumentStatusUpdate
from nautilus_trader.model.enums import BookLevel
from nautilus_trader.model.events.order import OrderAccepted
from nautilus_trader.model.events.order import OrderCanceled
from nautilus_trader.model.events.position import PositionChanged
from nautilus_trader.model.events.position import PositionClosed
from nautilus_trader.model.events.position import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orderbook.book import OrderBook
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.trading.strategy import TradingStrategy
from nautilus_trader.trading.strategy import TradingStrategyConfig


class TickTock(TradingStrategy):
    """
    A strategy to test correct sequencing of tick data and timers.
    """

    def __init__(self, instrument: Instrument, bar_type: BarType):
        """
        Initialize a new instance of the ``TickTock`` class.

        Parameters
        ----------
        instrument : Instrument
            The instrument for the strategy.
        bar_type : BarType
            The bar type for the strategy.

        """
        super().__init__()

        # Configuration
        self.instrument = instrument
        self.bar_type = bar_type

        self.store = []  # type: list[Any]
        self.timer_running = False
        self.time_alert_counter = 0

    def on_start(self):
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.bar_type.instrument_id)

    def on_quote_tick(self, tick):
        self.log.info(f"Received {repr(tick)}")
        self.store.append(tick)

    def on_bar(self, bar):
        self.log.info(f"Received {repr(bar)}")
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


class EMACrossConfig(TradingStrategyConfig):
    """
    Configuration for ``EMACross`` instances.

    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    trade_size : Decimal
        The position size per trade.
    fast_ema_period : int
        The fast EMA period.
    slow_ema_period : int
        The slow EMA period.
    """

    instrument_id: str
    bar_type: str
    trade_size: Decimal
    fast_ema_period: int = 10
    slow_ema_period: int = 20


class EMACross(TradingStrategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position in that
    direction.
    """

    def __init__(self, config: EMACrossConfig):
        """
        Initialize a new instance of the ``EMACross`` class.

        Parameters
        ----------
        config : EMACrossConfig
            The configuration for the instance.

        """
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.bar_type = BarType.from_str(config.bar_type)
        self.trade_size = config.trade_size

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

        self.instrument: Optional[Instrument] = None  # Initialized in on_start

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

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

    def on_bar(self, bar: Bar):
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(f"Received {repr(bar)}")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up " f"[{self.cache.bar_count(self.bar_type)}]...",
                color=LogColor.BLUE,
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
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def sell(self):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
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

    def on_save(self) -> Dict[str, bytes]:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {"example": b"123456"}

    def on_load(self, state: Dict[str, bytes]):
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """
        example = state["example"].decode()

        self.log.info(f"Loaded users state {example}")

    def on_dispose(self):
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
        self.unsubscribe_bars(self.bar_type)


class OrderBookImbalanceStrategyConfig(TradingStrategyConfig):
    """
    Configuration for ``OrderBookImbalance`` instances.

    instrument_id : InstrumentId
        The instrument ID for the strategy.
    trade_size : Decimal
        The position size per trade.
    """

    instrument_id: str
    trade_size: Decimal
    oms_type: str = "NETTING"


class OrderBookImbalanceStrategy(TradingStrategy):
    """
    A simple orderbook imbalance hitting strategy
    """

    def __init__(self, config: OrderBookImbalanceStrategyConfig):
        """
        Initialize a new instance of the ``OrderBookImbalanceStrategy`` class.

        Parameters
        ----------
        config: OrderBookImbalanceStrategyConfig
            The configuration for the instance.

        """
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.trade_size = config.trade_size

        self.instrument: Optional[Instrument] = None  # Initialized in on_start
        self.instrument_status: Optional[InstrumentStatus] = None
        self.close_price: Optional[InstrumentClosePrice] = None
        self.book: Optional[OrderBook] = None

    def on_start(self):
        """Actions to be performed on strategy start."""
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Create orderbook
        self.book = OrderBook.create(instrument=self.instrument, level=BookLevel.L2)

        # Subscribe to live data
        self.subscribe_order_book_deltas(self.instrument_id)
        self.subscribe_instrument_status_updates(self.instrument_id)
        self.subscribe_instrument_close_prices(self.instrument_id)

    def on_order_book_delta(self, delta: OrderBookData):
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        delta : OrderBookDelta, OrderBookDeltas, OrderBookSnapshot
            The order book delta received.

        """
        self.book.apply(delta)
        bid_qty = self.book.best_bid_qty()
        ask_qty = self.book.best_ask_qty()
        if bid_qty and ask_qty:
            imbalance = bid_qty / (bid_qty + ask_qty)
            if imbalance > 0.90:
                self.buy()
            elif imbalance < 0.10:
                self.sell()

    def buy(self):
        """
        Users simple buy method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def sell(self):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
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

    def on_instrument_status_update(self, data: InstrumentStatusUpdate):
        if data.status == InstrumentStatus.CLOSED:
            self.instrument_status = data.status

    def on_instrument_close_price(self, data: InstrumentClosePrice):
        self.close_price = data.close_price

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.

        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)


class MarketMaker(TradingStrategy):
    """
    Provides a market making strategy for testing.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        trade_size: Decimal,
        max_size: Decimal,
    ):
        """
        Initialize a new instance of the ``MarketMaker`` class.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID for the strategy.
        trade_size : Decimal
            The position size per trade.
        max_size : Decimal
            The maximum inventory size allowed.

        """
        super().__init__()

        # Configuration
        self.instrument_id = instrument_id
        self.trade_size = trade_size
        self.max_size = max_size

        self.instrument: Optional[Instrument] = None  # Initialized in on_start
        self._book: Optional[OrderBook] = None
        self._mid: Optional[Decimal] = None
        self._adj = Decimal(0)

    def on_start(self):
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Create orderbook
        self._book = OrderBook.create(instrument=self.instrument, level=BookLevel.L2)

        # Subscribe to live data
        self.subscribe_order_book_deltas(self.instrument_id)

    def on_order_book_delta(self, delta: OrderBookData):
        self._book.apply(delta)
        bid_price = self._book.best_ask_price()
        ask_price = self._book.best_ask_price()
        if bid_price and ask_price:
            mid = (bid_price + ask_price) / 2
            if mid != self._mid:
                self.cancel_all_orders(self.instrument_id)
                self._mid = Decimal(mid)
                val = self._mid + self._adj
                self.buy(price=val * Decimal(1.01))
                self.sell(price=val * Decimal(0.99))

    def on_event(self, event: Event):
        if isinstance(event, (PositionOpened, PositionChanged)):
            self._adj = (event.net_qty / self.max_size) * Decimal(0.01)
        elif isinstance(event, PositionClosed):
            self._adj = Decimal(0)

    def buy(self, price: Decimal):
        """
        Users simple buy method (example).
        """
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            price=Price(price, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def sell(self, price: Decimal):
        """
        Users simple sell method (example).
        """
        order = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            price=Price(price, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )

        self.submit_order(order)

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.

        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)


class RepeatedOrdersConfig(TradingStrategyConfig):
    """
    Configuration for ``RepeatedOrders`` instances.

    instrument_id : InstrumentId
        The instrument ID for the strategy.
    trade_size : Decimal
        The position size per trade.
    """

    instrument_id: str
    trade_size: Decimal


class RepeatedOrders(TradingStrategy):
    """
    Provides a repeated orders strategy for testing.
    """

    def __init__(self, config: RepeatedOrdersConfig):
        """
        Initialize a new instance of the ``RepeatedOrders`` class.

        Parameters
        ----------
        config : RepeatedOrdersConfig
            The configuration for the instance.

        """
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.trade_size = config.trade_size

        self.instrument: Optional[Instrument] = None  # Initialized in on_start
        self._last_sent = self.clock.utc_now()
        self._order_count = 0

    def on_start(self):
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        self.subscribe_order_book_deltas(instrument_id=self.instrument_id)

    def on_order_book_delta(self, data: OrderBookData):
        if not self.cache.orders_inflight():
            self.send_orders()

    def send_orders(self):
        self.log.debug("Checking order send")

        if self.cache.orders_working():
            self.log.debug("Order working, skipping")
            return

        if self.cache.orders_inflight():
            self.log.debug("Order inflight, skipping")
            return

        self.log.info("Sending order! ")

        buy = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            price=Price(MIN_BET_PROB, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )
        sell = self.order_factory.limit(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            price=Price(MAX_BET_PROB, precision=self.instrument.price_precision),
            quantity=self.instrument.make_qty(self.trade_size),
        )
        self.submit_order(buy)
        self.submit_order(sell)

    def on_event(self, event: Event):
        if isinstance(event, OrderAccepted):
            order = self.cache.order(event.client_order_id)
            self.log.info(f"Canceling order: {order}")
            self.cancel_order(order=order)
        elif isinstance(event, OrderCanceled):
            self.log.info("Got cancel, sending again")
            self.send_orders()

    def on_stop(self):
        """
        Actions to be performed when the strategy is stopped.

        """
        self.cancel_all_orders(self.instrument_id)
        self.flatten_all_positions(self.instrument_id)
