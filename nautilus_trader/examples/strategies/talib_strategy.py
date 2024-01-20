# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.indicators.ta_lib.manager import TAFunctionWrapper
from nautilus_trader.indicators.ta_lib.manager import TALibIndicatorManager
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


# *** THIS IS A TEST STRATEGY WITH NO ALPHA ADVANTAGE WHATSOEVER. ***
# *** IT IS NOT INTENDED TO BE USED TO TRADE LIVE WITH REAL MONEY. ***


class TALibStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for ``TALibStrategy`` instances.

    Parameters
    ----------
    bar_type : BarType
        The bar type for the strategy.

    """

    bar_type: BarType


class TALibStrategy(Strategy):
    """
    A trading strategy demonstration using TA-Lib (Technical Analysis Library) for
    generating trading signals based on technical indicators. This strategy is intended
    for educational purposes and does not execute real trading orders. Instead, it logs
    potential actions derived from technical analysis signals.

    This strategy is configured to use a variety of technical indicators such as EMA (Exponential
    Moving Averages), RSI (Relative Strength Index), and MACD (Moving Average Convergence Divergence).
    It demonstrates how these indicators can be utilized to identify potential trading opportunities
    based on market data.

    The strategy responds to incoming bar data (candlestick data) and analyzes it using the set
    indicators to make decisions. It can identify conditions like EMA crossovers, overbought or
    oversold RSI levels, and MACD histogram values to log potential buy or sell signals.

    Args
    ----
    config : TALibStrategyConfig
        The configuration object for the strategy, which includes the `bar_type` specifying the
        market data type (like minute bars, tick bars, etc.) to be used in the strategy.

    Attributes
    ----------
    instrument_id : InstrumentId
        The ID of the instrument (like a stock or currency pair) that the strategy operates on.
    bar_type : BarType
        The type of market data bars the strategy is configured to use.
    indicator_manager : TALibIndicatorManager
        Manages the indicators used in the strategy, handling their initialization, update,
        and value retrieval.

    """

    def __init__(self, config: TALibStrategyConfig) -> None:
        PyCondition.type(config.bar_type, BarType, "config.bar_type")
        super().__init__(config)

        # Configuration
        self.instrument_id = config.bar_type.instrument_id
        self.bar_type = config.bar_type

        # Create the indicators for the strategy
        self.indicator_manager: TALibIndicatorManager = TALibIndicatorManager(
            bar_type=self.bar_type,
            period=2,
        )

        # Specify the necessary indicators, configuring them as individual or grouped instances
        # in TALibIndicatorManager.  This approach uses string identifiers, each corresponding to
        # an indicator's output name, to instantiate TAFunctionWrappers
        indicators = [
            "ATR_14",
            "EMA_10",
            "EMA_20",
            "RSI_14",
            "MACD_12_26_9",
            "MACD_12_26_9_SIGNAL",
            "MACD_12_26_9_HIST",
        ]
        self.indicator_manager.set_indicators(TAFunctionWrapper.from_list_of_str(indicators))

        # Initialize on_start
        self.instrument: Instrument | None = None

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
        self.register_indicator_for_bars(self.bar_type, self.indicator_manager)

        # Subscribe to live data
        self.subscribe_bars(self.bar_type)
        self.subscribe_quote_ticks(self.instrument_id)

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

        # Check EMA cross-over
        if self.indicator_manager.value("EMA_10") > self.indicator_manager.value(
            "EMA_20",
            1,
        ) and self.indicator_manager.value("EMA_10", 1) < self.indicator_manager.value("EMA_20"):
            self.log.info("EMA_10 crossed above EMA_20", color=LogColor.GREEN)
        elif self.indicator_manager.value("EMA_10") < self.indicator_manager.value(
            "EMA_20",
            1,
        ) and self.indicator_manager.value("EMA_10", 1) > self.indicator_manager.value("EMA_20"):
            self.log.info("EMA_10 crossed below EMA_20", color=LogColor.GREEN)

        # Check RSI
        if self.indicator_manager.value("RSI_14") > 70:
            self.log.info("RSI_14 is overbought", color=LogColor.MAGENTA)
        elif self.indicator_manager.value("RSI_14") < 30:
            self.log.info("RSI_14 is oversold", color=LogColor.MAGENTA)

        # Check MACD Histogram
        if self.indicator_manager.value("MACD_12_26_9_HIST") > 0:
            self.log.info("MACD_12_26_9_HIST is positive", color=LogColor.MAGENTA)
        elif self.indicator_manager.value("MACD_12_26_9_HIST") < 0:
            self.log.info("MACD_12_26_9_HIST is negative", color=LogColor.MAGENTA)

    def on_stop(self) -> None:
        """
        Actions to be performed when the strategy is stopped.
        """
        # Unsubscribe from data
        self.unsubscribe_bars(self.bar_type)
