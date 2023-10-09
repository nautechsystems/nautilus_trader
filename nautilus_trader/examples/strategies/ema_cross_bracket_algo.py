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

from datetime import timedelta
from decimal import Decimal
from typing import Any, Optional

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
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import ExecAlgorithmId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.objects import Price
from nautilus_trader.model.orders import OrderList
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class EMACrossBracketAlgoConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``EMACrossBracketAlgo`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    trade_size : str
        The position size per trade (interpreted as Decimal).
    atr_period : int, default 20
        The period for the ATR indicator.
    fast_ema_period : int, default 10
        The fast EMA period.
    slow_ema_period : int, default 20
        The slow EMA period.
    bracket_distance_atr : float, default 3.0
        The SL and TP bracket distance from entry ATR multiple.
    emulation_trigger : str, default 'NO_TRIGGER'
        The emulation trigger for submitting emulated orders.
        If ``None`` then orders will not be emulated.
    entry_exec_algorithm_id : str, optional
        The execution algorithm for entry orders.
    entry_exec_algorithm_params : dict[str, Any], optional
        The execution algorithm params for entry orders.
    sl_exec_algorithm_id : str, optional
        The execution algorithm for stop-loss (SL) orders.
    sl_exec_algorithm_params : dict[str, Any], optional
        The execution algorithm params for stop-loss (SL) orders.
    tp_exec_algorithm_id : str, optional
        The execution algorithm for take-profit (TP) orders.
    tp_exec_algorithm_params : dict[str, Any], optional
        The execution algorithm params for take-profit (TP) orders.
    close_positions_on_stop : bool, default True
        If all open positions should be closed on strategy stop.
    order_id_tag : str
        The unique order ID tag for the strategy. Must be unique
        amongst all running strategies for a particular trader ID.
    oms_type : OmsType
        The order management system type for the strategy. This will determine
        how the `ExecutionEngine` handles position IDs (see docs).
    manage_gtd_expiry : bool, default True
        If all order GTD time in force expirations should be managed by the strategy.

    """

    instrument_id: str
    bar_type: str
    trade_size: Decimal
    atr_period: int = 20
    fast_ema_period: int = 10
    slow_ema_period: int = 20
    bracket_distance_atr: float = 3.0
    emulation_trigger: str = "NO_TRIGGER"
    entry_exec_algorithm_id: Optional[str] = None
    entry_exec_algorithm_params: Optional[dict[str, Any]] = None
    sl_exec_algorithm_id: Optional[str] = None
    sl_exec_algorithm_params: Optional[dict[str, Any]] = None
    tp_exec_algorithm_id: Optional[str] = None
    tp_exec_algorithm_params: Optional[dict[str, Any]] = None
    close_positions_on_stop: bool = True


class EMACrossBracketAlgo(Strategy):
    """
    A simple moving average cross example strategy.

    When the fast EMA crosses the slow EMA then enter a position at the market
    in that direction.

    Cancels all orders and closes all positions on stop.

    Parameters
    ----------
    config : EMACrossConfig
        The configuration for the instance.

    Raises
    ------
    ValueError
        If `config.fast_ema_period` is not less than `config.slow_ema_period`.

    """

    def __init__(self, config: EMACrossBracketAlgoConfig) -> None:
        PyCondition.true(
            config.fast_ema_period < config.slow_ema_period,
            "{config.fast_ema_period=} must be less than {config.slow_ema_period=}",
        )
        super().__init__(config)

        # Configuration
        self.instrument_id = InstrumentId.from_str(config.instrument_id)
        self.bar_type = BarType.from_str(config.bar_type)
        self.bracket_distance_atr = config.bracket_distance_atr
        self.trade_size = Decimal(config.trade_size)
        self.emulation_trigger = TriggerType[config.emulation_trigger]

        # Create the indicators for the strategy
        self.atr = AverageTrueRange(config.atr_period)
        self.fast_ema = ExponentialMovingAverage(config.fast_ema_period)
        self.slow_ema = ExponentialMovingAverage(config.slow_ema_period)

        # Order management
        self.entry_exec_algorithm_id = (
            ExecAlgorithmId(config.entry_exec_algorithm_id)
            if config.entry_exec_algorithm_id is not None
            else None
        )
        self.entry_exec_algorithm_params = config.entry_exec_algorithm_params

        self.sl_exec_algorithm_id = (
            ExecAlgorithmId(config.sl_exec_algorithm_id)
            if config.sl_exec_algorithm_id is not None
            else None
        )
        self.sl_exec_algorithm_params = config.sl_exec_algorithm_params

        self.tp_exec_algorithm_id = (
            ExecAlgorithmId(config.tp_exec_algorithm_id)
            if config.tp_exec_algorithm_id is not None
            else None
        )
        self.tp_exec_algorithm_params = config.tp_exec_algorithm_params

        self.close_positions_on_stop = config.close_positions_on_stop
        self.instrument: Optional[Instrument] = None  # Initialized in on_start

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.instrument_id}")
            self.stop()
            return

        # Register the indicators for updating
        self.register_indicator_for_bars(self.bar_type, self.atr)
        self.register_indicator_for_bars(self.bar_type, self.fast_ema)
        self.register_indicator_for_bars(self.bar_type, self.slow_ema)

        # Get historical data
        self.request_bars(self.bar_type)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.instrument_id)

    def on_quote_tick(self, tick: QuoteTick) -> None:
        """
        Actions to be performed when the strategy is running and receives a quote tick.

        Parameters
        ----------
        tick : QuoteTick
            The tick received.

        """
        # For debugging (must add a subscription)
        # self.log.info(repr(tick), LogColor.CYAN)

    def on_bar(self, bar: Bar) -> None:
        """
        Actions to be performed when the strategy is running and receives a bar.

        Parameters
        ----------
        bar : Bar
            The bar received.

        """
        self.log.info(repr(bar), LogColor.CYAN)

        # Check if indicators ready
        if not self.indicators_initialized():
            self.log.info(
                f"Waiting for indicators to warm up [{self.cache.bar_count(self.bar_type)}]...",
                color=LogColor.BLUE,
            )
            return  # Wait for indicators to warm up...

        if bar.is_single_price():
            # Implies no market information for this bar
            return

        # BUY LOGIC
        if self.fast_ema.value >= self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.cancel_all_orders(self.instrument_id)
                self.buy(bar)
            elif self.portfolio.is_net_short(self.instrument_id):
                self.close_all_positions(self.instrument_id)
                self.cancel_all_orders(self.instrument_id)
                self.buy(bar)
        # SELL LOGIC
        elif self.fast_ema.value < self.slow_ema.value:
            if self.portfolio.is_flat(self.instrument_id):
                self.cancel_all_orders(self.instrument_id)
                self.sell(bar)
            elif self.portfolio.is_net_long(self.instrument_id):
                self.close_all_positions(self.instrument_id)
                self.cancel_all_orders(self.instrument_id)
                self.sell(bar)

    def buy(self, last_bar: Bar) -> None:
        """
        Users bracket buy method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        tick_size: Price = self.instrument.price_increment
        bracket_distance: float = self.bracket_distance_atr * self.atr.value

        order_list: OrderList = self.order_factory.bracket(
            instrument_id=self.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.trade_size),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + timedelta(seconds=30),
            entry_trigger_price=self.instrument.make_price(last_bar.close + tick_size),
            sl_trigger_price=self.instrument.make_price(last_bar.close - bracket_distance),
            tp_price=self.instrument.make_price(last_bar.close + bracket_distance),
            entry_order_type=OrderType.MARKET_IF_TOUCHED,
            emulation_trigger=self.emulation_trigger,
            entry_exec_algorithm_id=self.entry_exec_algorithm_id,
            entry_exec_algorithm_params=self.entry_exec_algorithm_params,
            sl_exec_algorithm_id=self.sl_exec_algorithm_id,
            sl_exec_algorithm_params=self.sl_exec_algorithm_params,
            tp_exec_algorithm_id=self.tp_exec_algorithm_id,
            tp_exec_algorithm_params=self.tp_exec_algorithm_params,
        )

        self.submit_order_list(order_list)

    def sell(self, last_bar: Bar) -> None:
        """
        Users bracket sell method (example).
        """
        if not self.instrument:
            self.log.error("No instrument loaded.")
            return

        tick_size: Price = self.instrument.price_increment
        bracket_distance: float = self.bracket_distance_atr * self.atr.value

        order_list: OrderList = self.order_factory.bracket(
            instrument_id=self.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.trade_size),
            time_in_force=TimeInForce.GTD,
            expire_time=self.clock.utc_now() + timedelta(seconds=30),
            entry_trigger_price=self.instrument.make_price(last_bar.low - tick_size),
            sl_trigger_price=self.instrument.make_price(last_bar.close + bracket_distance),
            tp_price=self.instrument.make_price(last_bar.close - bracket_distance),
            entry_order_type=OrderType.MARKET_IF_TOUCHED,
            emulation_trigger=self.emulation_trigger,
            entry_exec_algorithm_id=self.entry_exec_algorithm_id,
            entry_exec_algorithm_params=self.entry_exec_algorithm_params,
            sl_exec_algorithm_id=self.sl_exec_algorithm_id,
            sl_exec_algorithm_params=self.sl_exec_algorithm_params,
            tp_exec_algorithm_id=self.tp_exec_algorithm_id,
            tp_exec_algorithm_params=self.tp_exec_algorithm_params,
        )

        self.submit_order_list(order_list)

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

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.instrument_id)
        self.close_all_positions(self.instrument_id)

        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)
        self.unsubscribe_quote_ticks(self.instrument_id)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        # Reset indicators here
        self.fast_ema.reset()
        self.slow_ema.reset()

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
