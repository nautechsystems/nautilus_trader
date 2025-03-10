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

import types

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


class DemoStrategyConfig(StrategyConfig, frozen=True):
    instrument: Instrument
    bar_type: BarType


# Signal names for price extremes
signals = types.SimpleNamespace()
signals.NEW_HIGHEST_PRICE = "NewHighestPriceReached"
signals.NEW_LOWEST_PRICE = "NewLowestPriceReached"


class DemoStrategy(Strategy):
    """
    A demonstration strategy showing how to use Actor-Based Signal messaging.

    This example demonstrates the simplest messaging approach in NautilusTrader:
    - Using signals for lightweight notifications (price extremes in this case)
    - Publishing signals with single string values
    - Subscribing to signals and handling them in on_signal

    Two signals are used:
    - NewHighestPriceReached: Published when a new maximum price is detected
    - NewLowestPriceReached: Published when a new minimum price is detected

    """

    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config)

        # Initialize price tracking
        self.highest_price = float("-inf")
        self.lowest_price = float("inf")

    def on_start(self):
        # Subscribe to market data
        self.subscribe_bars(self.config.bar_type)
        self.log.info(f"Subscribed to {self.config.bar_type}", color=LogColor.YELLOW)

        # Subscribe to signals - each signal subscription will trigger on_signal when that signal is published
        self.subscribe_signal(signals.NEW_HIGHEST_PRICE)
        self.subscribe_signal(signals.NEW_LOWEST_PRICE)
        self.log.info("Subscribed to price extreme signals", color=LogColor.YELLOW)

    def on_bar(self, bar: Bar):
        # Check for new highest price
        if bar.close > self.highest_price:
            self.highest_price = bar.close
            self.log.info(f"New highest price detected: {bar.close}")
            # Publish a lightweight signal (can only contain a single value of type str/int/float)
            self.publish_signal(
                name=signals.NEW_HIGHEST_PRICE,  # Signal name
                value=signals.NEW_HIGHEST_PRICE,  # Signal value
                ts_event=bar.ts_event,
            )

        # Check for new lowest price
        if bar.close < self.lowest_price:
            self.lowest_price = bar.close
            self.log.info(f"New lowest price detected: {bar.close}")
            # Publish a lightweight signal (can only contain a single value of type str/int/float)
            self.publish_signal(
                name=signals.NEW_LOWEST_PRICE,
                value=signals.NEW_LOWEST_PRICE,  # Using same string as name for simplicity
                ts_event=bar.ts_event,
            )

    def on_signal(self, signal):
        """
        Handle incoming signals.

        This method is automatically called when any signal we're subscribed to is published.
        Important: In the signal handler, we can only match against signal.value
        (signal.name is not accessible in the handler).

        """
        match signal.value:
            case signals.NEW_HIGHEST_PRICE:
                self.log.info(
                    f"New highest price was reached. | "
                    f"Signal value: {signal.value} | "
                    f"Signal time: {unix_nanos_to_dt(signal.ts_event)}",
                    color=LogColor.GREEN,
                )
            case signals.NEW_LOWEST_PRICE:
                self.log.info(
                    f"New lowest price was reached. | "
                    f"Signal value: {signal.value} | "
                    f"Signal time: {unix_nanos_to_dt(signal.ts_event)}",
                    color=LogColor.RED,
                )

    def on_stop(self):
        self.log.info("Strategy stopped.")
