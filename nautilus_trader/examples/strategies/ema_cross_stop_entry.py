# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators.atr import AverageTrueRange
from nautilus_trader.indicators.average.ema import ExponentialMovingAverage
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orderbook import OrderBook
from nautilus_trader.model.orders import MarketIfTouchedOrder
from nautilus_trader.model.orders import TrailingStopMarketOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class EMACrossStopEntryConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``EMACrossStopEntry`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    atr_period : int
        The period for the ATR indicator.
    trailing_atr_multiple : float
        The ATR multiple for the trailing stop.
    trailing_offset_type : str
        The trailing offset type (interpreted as `TrailingOffsetType`).
    trailing_offset : Decimal
        The trailing offset amount.
    trigger_type : str
        The trailing stop trigger type (interpreted as `TriggerType`).
    trade_size : str
        The position size per trade (interpreted as Decimal).
    fast_ema_period : int, default 10
        The fast EMA period.
    slow_ema_period : int, default 20
        The slow EMA period.
    emulation_trigger : str, default 'NO_TRIGGER'
        The emulation trigger for submitting emulated orders.
        If 'NONE' then orders will not be emulated.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OmsType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).

    """

    instrument_id: str
    bar_type: str
    atr_period: int
    trailing_atr_multiple: float
    trailing_offset_type: str
    trailing_offset: Decimal
    trigger_type: str
    trade_size: Decimal
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    emulation_trigger: str = "NO_TRIGGER"


class EMACrossStopEntry(Strategy):
    """
    A simple moving average cross example strategy with a `MARKET_IF_TOUCHED` entry and
    `TRAILING_STOP_MARKET` stop.

    When the fast EMA crosses the slow EMA then submits a `MARKET_IF_TOUCHED` order
    one tick above the current bar for BUY, or one tick below the current bar
    for SELL.

    If the entry order is filled then a `TRAILING_STOP_MARKET` at a specified
    ATR distance is submitted and managed.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : EMACrossStopEntryConfig
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `config.fast_ema_period` is not less than `config.slow_ema_period`.

    """

    def __init__(self, config: EMACrossStopEntryConfig) -> None:
        PyCondition.true(
            config.fast_ema_period < config.slow_ema_period,
            "{config.fast_ema_period=} must be less than {config.slow_ema_period=}",
        )
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.bar_type = BarType.from_str(config.bar_type)
        self.trade_size = Decimal(config.trade_size)
        self.trailing_atr_multiple = config.trailing_atr_multiple
        self.trailing_offset_type = TrailingOffsetType[config.trailing_offset_type]
        self.trailing_offset = config.trailing_offset
        self.trigger_type = TriggerType[config.trigger_type]
        self.emulation_trigger = TriggerType[config.emulation_trigger]

        # Create the indicators for the strategy
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)
        self.atr = AverageTrueRange(config.atr_period)

        self.instrument: Optional[Instrument] = None  # Initialized in `on_start()`
        self.tick_size = None  # Initialized in `on_start()`

        # Users order management variables
        self.entry = None
        self.trailing_stop = None

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        self.tick_size = self.instrument.price_increment

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)
        self.register_indicator_for_bars(self.bar_type, self.atr)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.instrument_id)
        self.subscribe_trade_ticks(self.instrument_id)

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
        self.log.info(f"Received {bar!r}")

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up [{self.cache.bar_count(self.bar_type)}]...",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        if self.portfolio.is_flat(self.instrument_id):
            if self.entry is not None:
                self.cancel_order(self.entry)

            # BUY LOGIC
            if self.fast_ema.value >= self.slow_ema.value:
                self.entry_buy(bar)
            # SELL LOGIC
            else:  # fast_ema.value < self.slow_ema.value
                self.entry_sell(bar)

    def entry_buy(self, last_bar: Bar) -> None:
        """
        Users simple buy entry method (example).

        Parameters
        ----------
        last_bar : Bar
            The last bar received.

        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        order: MarketIfTouchedOrder = self.order_factory.market_if_touched(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            time_in_force=TimeInForce.IOC,
            trigger_price=self.instrument.make_price(last_bar.high + (self.tick_size * 2)),
            emulation_trigger=self.emulation_trigger,
        )
        # TODO(cs): Uncomment below order for development
        # order: LimitIfTouchedOrder = self.order_factory.limit_if_touched(
        #     instrument_id=self.instrument_id,
        #     order_side=OrderSide.BUY,
        #     quantity=self.instrument.make_qty(self.trade_size),
        #     time_in_force=TimeInForce.IOC,
        #     price=self.instrument.make_price(last_bar.low - (self.tick_size * 2)),
        #     trigger_price=self.instrument.make_price(last_bar.high + (self.tick_size * 2)),
        # )

        self.entry = order
        self.submit_order(order)

    def entry_sell(self, last_bar: Bar) -> None:
        """
        Users simple sell entry method (example).

        Parameters
        ----------
        last_bar : Bar
            The last bar received.

        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        order: MarketIfTouchedOrder = self.order_factory.market_if_touched(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            time_in_force=TimeInForce.IOC,
            trigger_price=self.instrument.make_price(last_bar.low - (self.tick_size * 2)),
            emulation_trigger=self.emulation_trigger,
        )
        # TODO(cs): Uncomment below order for development
        # order: LimitIfTouchedOrder = self.order_factory.limit_if_touched(
        #     instrument_id=self.instrument_id,
        #     order_side=OrderSide.SELL,
        #     quantity=self.instrument.make_qty(self.trade_size),
        #     time_in_force=TimeInForce.IOC,
        #     price=self.instrument.make_price(last_bar.low - (self.tick_size * 2)),
        #     trigger_price=self.instrument.make_price(last_bar.low - (self.tick_size * 2)),
        # )

        self.entry = order
        self.submit_order(order)

    def trailing_stop_buy(self) -> None:
        """
        Users simple trailing stop BUY for (``SHORT`` positions).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        offset = self.atr.value * self.trailing_atr_multiple
        order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            trailing_offset=Decimal(f"{offset:.{self.instrument.price_precision}f}"),
            trailing_offset_type=self.trailing_offset_type,
            trigger_type=self.trigger_type,
            reduce_only=True,
            emulation_trigger=self.emulation_trigger,
        )

        self.trailing_stop = order
        self.submit_order(order)

    def trailing_stop_sell(self) -> None:
        """
        Users simple trailing stop SELL for (LONG positions).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        offset = self.atr.value * self.trailing_atr_multiple
        order: TrailingStopMarketOrder = self.order_factory.trailing_stop_market(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            trailing_offset=Decimal(f"{offset:.{self.instrument.price_precision}f}"),
            trailing_offset_type=self.trailing_offset_type,
            trigger_type=self.trigger_type,
            reduce_only=True,
            emulation_trigger=self.emulation_trigger,
        )

        self.trailing_stop = order
        self.submit_order(order)

    def on_data(self, data: Data) -> None:
        """
        Actions to be performed when the strategy is running and receives generic data.

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
            if self.entry and event.client_order_id == self.entry.client_order_id:
                if event.order_side == OrderSide.BUY:
                    self.trailing_stop_sell()
                elif event.order_side == OrderSide.SELL:
                    self.trailing_stop_buy()
            if self.trailing_stop and event.client_order_id == self.trailing_stop.client_order_id:
                self.trailing_stop = None

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.instrument_id)
        self.unsubscribe_trade_ticks(self.instrument_id)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()
        self.atr.reset()

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
