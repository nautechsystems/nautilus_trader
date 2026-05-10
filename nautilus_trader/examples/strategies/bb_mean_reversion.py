# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.core.data import Data
from nautilus_trader.core.message import Event
from nautilus_trader.indicators import BollingerBands
from nautilus_trader.indicators import RelativeStrengthIndex
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.model.orders import MarketOrder
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class BBMeanReversionConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``BBMeanReversion`` instances.

    Parameters
    ----------
    instrument_id : InstrumentId
        The instrument ID for the strategy.
    bar_type : BarType
        The bar type for the strategy.
    trade_size : Decimal
        The position size per trade.
    bb_period : int, default 20
        The Bollinger Bands rolling window period.
    bb_std : float, default 2.0
        The Bollinger Bands standard deviation multiple.
    rsi_period : int, default 14
        The RSI rolling window period.
    rsi_buy_threshold : float, default 0.30
        The RSI threshold below which a buy signal is valid (range 0-1).
    rsi_sell_threshold : float, default 0.70
        The RSI threshold above which a sell signal is valid (range 0-1).
    close_positions_on_stop : bool, default True
        If all open positions should be closed on strategy stop.

    """

    instrument_id: InstrumentId
    bar_type: BarType
    trade_size: Decimal
    bb_period: PositiveInt = 20
    bb_std: PositiveFloat = 2.0
    rsi_period: PositiveInt = 14
    rsi_buy_threshold: float = 0.30
    rsi_sell_threshold: float = 0.70
    close_positions_on_stop: bool = True


class BBMeanReversion(Strategy):
    """
    A Bollinger Band mean reversion example strategy.

    When price touches the lower band with RSI confirmation, enter long.
    When price touches the upper band with RSI confirmation, enter short.
    Exit positions when price reverts to the middle band.

    Parameters
    ----------
    config : BBMeanReversionConfig
        The configuration for the instance.

    """

    def __init__(self, config: BBMeanReversionConfig) -> None:
        super().__init__(config)

        self.instrument: Instrument = None

        # Create the indicators for the strategy
        self.bb = BollingerBands(config.bb_period, config.bb_std)
        self.rsi = RelativeStrengthIndex(config.rsi_period)

    def on_start(self) -> None:
        """
        Actions to be performed on strategy start.
        """
        self.instrument = self.cache.instrument(self.config.instrument_id)
        if self.instrument is None:
            self.log.error(f"Could not find instrument for {self.config.instrument_id}")
            self.stop()
            return

        # Register the indicators for updating
        self.register_indicator_for_bars(self.config.bar_type, self.bb)
        self.register_indicator_for_bars(self.config.bar_type, self.rsi)

        # Subscribe to bar data
        self.subscribe_bars(self.config.bar_type)

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
                f"Waiting for indicators to warm up [{self.cache.bar_count(self.config.bar_type)}]",
                color=LogColor.BLUE,
            )
            return

        if bar.is_single_price():
            return

        close = bar.close.as_double()

        if not self._check_exit(close):
            self._check_entry(close)

    def _check_exit(self, close: float) -> bool:
        iid = self.config.instrument_id
        if self.portfolio.is_net_long(iid) and close >= self.bb.middle:
            self.close_all_positions(iid)
            return True
        if self.portfolio.is_net_short(iid) and close <= self.bb.middle:
            self.close_all_positions(iid)
            return True
        return False

    def _check_entry(self, close: float) -> None:
        iid = self.config.instrument_id
        if close <= self.bb.lower and self.rsi.value < self.config.rsi_buy_threshold:
            if self.portfolio.is_net_short(iid):
                self.close_all_positions(iid)
            if not self.portfolio.is_net_long(iid):
                self.buy()
        elif close >= self.bb.upper and self.rsi.value > self.config.rsi_sell_threshold:
            if self.portfolio.is_net_long(iid):
                self.close_all_positions(iid)
            if not self.portfolio.is_net_short(iid):
                self.sell()

    def buy(self) -> None:
        """
        Users simple buy method (example).
        """
        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.BUY,
            quantity=self.instrument.make_qty(self.config.trade_size),
            time_in_force=TimeInForce.GTC,
        )

        self.submit_order(order)

    def sell(self) -> None:
        """
        Users simple sell method (example).
        """
        order: MarketOrder = self.order_factory.market(
            instrument_id=self.config.instrument_id,
            order_side=OrderSide.SELL,
            quantity=self.instrument.make_qty(self.config.trade_size),
            time_in_force=TimeInForce.GTC,
        )

        self.submit_order(order)

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

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        self.cancel_all_orders(self.config.instrument_id)
        if self.config.close_positions_on_stop:
            self.close_all_positions(self.config.instrument_id)

        self.unsubscribe_bars(self.config.bar_type)

    def on_reset(self) -> None:
        """
        Actions to be performed when the strategy is reset.
        """
        self.bb.reset()
        self.rsi.reset()

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
