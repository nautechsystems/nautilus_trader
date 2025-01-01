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
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import PositionChanged
from nautilus_trader.model.events import PositionClosed
from nautilus_trader.model.events import PositionOpened
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class EMACrossTrailingStopConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``EMACrossTrailingStop`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    atr_period : PositiveInt
        The period for the ATR indicator.
    trailing_atr_multiple : PositiveFloat
        The ATR multiple for the trailing stop.
    trailing_offset_type : str
        The trailing offset type (interpreted as `TrailingOffsetType`).
    trigger_type : str
        The trailing stop trigger type (interpreted as `TriggerType`).
    trade_size : Decimal
        The position size per trade.
    fast_ema_period : PositiveInt, default 10
        The fast EMA period.
    slow_ema_period : PositiveInt, default 20
        The slow EMA period.
    emulation_trigger : str, default 'NO_TRIGGER'
        The emulation trigger for submitting emulated orders.
        If 'NONE' then orders will not be emulated.

    """

    instrument_id: InstrumentId
    bar_type: BarType
    atr_period: PositiveInt
    trailing_atr_multiple: PositiveFloat
    trailing_offset_type: str
    trigger_type: str
    trade_size: Decimal
    fast_ema_period: PositiveInt = 10
    slow_ema_period: PositiveInt = 20
    emulation_trigger: str = "NO_TRIGGER"


class EMACrossTrailingStop(Strategy):
    """
    A simple moving average cross example strategy with a stop-market entry and trailing
    stop.

    When the fast EMA crosses the slow EMA then submits a stop-market order one
    tick above the current bar for BUY, or one tick below the current bar
    for SELL.

    If the entry order is filled then a trailing stop at a specified ATR
    distance is submitted and managed.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : EMACrossTrailingStopConfig
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `config.fast_ema_period` is not less than `config.slow_ema_period`.

    """

    def __init__(self, config: EMACrossTrailingStopConfig) -> None:
        PyCondition.is_true(
            config.fast_ema_period < config.slow_ema_period,
            "{config.fast_ema_period=} must be less than {config.slow_ema_period=}",
        )
        super().__init__(config)

        # Initialized in on_start
        self.instrument: Instrument | None = None
        self.tick_size = None

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)
        self.atr = AverageTrueRange(config.atr_period)

        # Users order management variables
        self.entry = None
        self.trailing_stop = None
        self.position_id = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        self.tick_size = self.instrument.price_increment

        # Register the indicators for updating
        self.register_indicator_for_bars(self.config.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.slow_ema)
        self.register_indicator_for_bars(self.config.bar_type, self.atr)

        # Get historical data
        self.request_bars(self.config.bar_type)

        # Subscribe to live data
        self.subscribe_quote_ticks(self.config.instrument_id)
        self.subscribe_bars(self.config.bar_type)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.config.instrument_id)
        self.close_all_positions(self.config.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_quote_ticks(self.config.instrument_id)
        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()
        self.atr.reset()

    def on_instrument(self, instrument: Instrument) -> None:
        """
        Actions to be performed when the strategy is running and receives an instrument.

        Parameters
        ----------
        instrument : Instrument
            The instrument received.

        """

    def on_order_book(self, order_book: OrderBook) -> None:
        """
        Actions to be performed when the strategy is running and receives an order book.

        Parameters
        ----------
        order_book : OrderBook
            The order book received.

        """
        # self.log.info(f"Received {order_book}")  # For debugging (must add a subscription)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """

    def on_trade_tick(self, tick: TradeTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a trade tick.

        Parameters
        ----------
        tick : TradeTick
            The tick received.

        """

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        # self.log.info(f"Received {bar!r}")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up [{self.cache.bar_count(self.config.bar_type)}]",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        if self.portfolio.is_flat(self.config.instrument_id):
            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                self.entry_buy()
            # SELL LOGIC
            else:  # fast_ema.value < self.slow_ema.value
                self.entry_sell()

    def entry_buy(self) -> None:
        """
        Users simple buy entry method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.config.trade_size),
        )

        self.entry = order
        self.submit_order(order)

    def entry_sell(self) -> None:
        """
        Users simple sell entry method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.trade_size),
        )

        self.entry = order
        self.submit_order(order)

    def trailing_stop_buy(self) -> None:
        """
        Users simple trailing stop BUY for (``SHORT`` positions).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        last_quote = self.cache.quote_tick(self.config.instrument_id)
        if not last_quote:
            self.log.warning("Cannot submit order: no quotes yet")
            return

        offset = self.atr.value * self.config.trailing_atr_multiple
        order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.config.trade_size),
            # limit_offset=Decimal(f"{offset / 2:.{self.instrument.price_precision}f}"),
            # price=self.instrument.make_price(last_quote.ask_price.as_double() + offset),
            trailing_offset=Decimal(f"{offset:.{self.instrument.price_precision}f}"),
            trailing_offset_type=TrailingOffsetType[self.config.trailing_offset_type],
            trigger_type=TriggerType[self.config.trigger_type],
            reduce_only=True,
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.trailing_stop = order
        self.submit_order(order, position_id=self.position_id)

    def trailing_stop_sell(self) -> None:
        """
        Users simple trailing stop SELL for (LONG positions).
        """
        if not self.instrument:
            self.log.error("No instrument loaded")
            return

        last_quote = self.cache.quote_tick(self.config.instrument_id)
        if not last_quote:
            self.log.warning("Cannot submit order: no quotes yet")
            return

        offset = self.atr.value * self.config.trailing_atr_multiple
        order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.trade_size),
            # limit_offset=Decimal(f"{offset / 2:.{self.instrument.price_precision}f}"),
            # price=self.instrument.make_price(last_quote.bid_price.as_double() - offset),
            trailing_offset=Decimal(f"{offset:.{self.instrument.price_precision}f}"),
            trailing_offset_type=TrailingOffsetType[self.config.trailing_offset_type],
            trigger_type=TriggerType[self.config.trigger_type],
            reduce_only=True,
            emulation_trigger=TriggerType[self.config.emulation_trigger],
        )

        self.trailing_stop = order
        self.submit_order(order, position_id=self.position_id)

    def on_data(self, data: Data) -> None:
        """
        Actions to be performed when the strategy is running and receives data.

        Parameters
        ----------
        data : Data
            The data received.

        """

    def on_event(self, event: Event) -> None:
        """
        Actions to be performed when the strategy is running and receives an event.

        Parameters
        ----------
        event : Event
            The event received.

        """
        if isinstance(event, OrderFilled):
            if self.trailing_stop and event.client_order_id == self.trailing_stop.client_order_id:
                self.trailing_stop = None
        elif isinstance(event, PositionOpened | PositionChanged):
            if self.trailing_stop:
                return  # Already a trailing stop
            if self.entry and event.opening_order_id == self.entry.client_order_id:
                if event.entry == OrderSide.BUY:
                    self.position_id = event.position_id
                    self.trailing_stop_sell()
                elif event.entry == OrderSide.SELL:
                    self.position_id = event.position_id
                    self.trailing_stop_buy()
        elif isinstance(event, PositionClosed):
            self.position_id = None

    def on_save(self) -> dict[str, bytes]:
        """
        Actions to be performed when the strategy is saved.

        Create and return a state dictionary of values to be saved.

        Returns
        -------
        dict[str, bytes]
            The strategy state dictionary.

        """
        return {}

    def on_load(self, state: dict[str, bytes]) -> None:
        """
        Actions to be performed when the strategy is loaded.

        Saved state values will be contained in the give state dictionary.

        Parameters
        ----------
        state : dict[str, bytes]
            The strategy state dictionary.

        """

    def on_dispose(self) -> None:
        """
        Actions to be performed when the strategy is disposed.

        Cleanup any resources used by the strategy here.

        """
