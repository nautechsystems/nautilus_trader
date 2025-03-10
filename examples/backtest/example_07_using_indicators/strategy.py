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

import datetime as dt
from collections import deque

from nautilus_trader.common.enums import LogColor
from nautilus_trader.indicators.average.ma_factory import MovingAverageFactory
from nautilus_trader.indicators.average.ma_factory import MovingAverageType
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.trading.strategy import Strategy


class DemoStrategy(Strategy):
    """
    A simple demonstration strategy showing how to use technical indicators in
    NautilusTrader.
    """

    def __init__(self, bar_type: BarType):
        super().__init__()

        # The bar_type parameter defines what market data we'll be working with
        self.bar_type = bar_type

        # Simple counter to track how many bars we've processed
        self.bars_processed = 0

        # Create a 10-period Exponential Moving Average (EMA) indicator
        self.ema10 = MovingAverageFactory.create(10, MovingAverageType.EXPONENTIAL)

        # Create a storage for the most recent 100 EMA values
        # We use Python's deque (double ended queue), because:
        # 1. It automatically removes old values when we reach 100 items
        # 2. It's very fast when adding new values at the start
        # 3. It keeps data in chronological order
        self.ema10_history: deque[float] = deque(maxlen=100)

        # Strategy execution timestamps for performance tracking
        self.start_time = None
        self.end_time = None

    def on_start(self):
        # Record strategy start time
        self.start_time = dt.datetime.now()
        self.log.info(f"Strategy started at: {self.start_time}")

        # Subscribe to market data
        # This tells NautilusTrader what data we want to receive in our on_bar method
        # Without this subscription, we won't receive any market data updates
        self.subscribe_bars(self.bar_type)
        self.log.info(f"Subscribed to {self.bar_type}")

        # Connect our EMA indicator to the market data stream
        # This is a key NautilusTrader feature that:
        # 1. Automatically updates the indicator when new bars arrive
        # 2. Ensures indicator values are current before our on_bar method is called
        # 3. Maintains proper data synchronization
        self.register_indicator_for_bars(self.bar_type, self.ema10)
        self.log.info("EMA(10) indicator registered")

    def on_bar(self, bar: Bar):
        # Track the number of bars we've processed
        self.bars_processed += 1

        # Thanks to our indicator registration in on_start,
        # self.ema10.value is already updated with the latest calculation

        # Most indicators need some initial data before they can produce valid results
        # For EMA(10), we need 10 bars of data to initialize the calculation
        if self.ema10.initialized:
            # Store the new EMA value in our historical record
            # We use `deque.appendleft()` to maintain a consistent order where:
            # - index [0] = latest value (newest)
            # - index [1] = previous value
            # - index [2] = two bars ago
            # This matches how NautilusTrader's Cache stores bar data
            self.ema10_history.appendleft(self.ema10.value)

            # Log current market data and indicator value
            self.log.info(
                f"Bar #{self.bars_processed} | "
                f"Close: {bar.close} | "
                f"EMA(10): {self.ema10.value:.5f}",
                color=LogColor.YELLOW,
            )

            # This demonstrates how to access previously calculated historical indicator values
            if len(self.ema10_history) > 1:
                previous_ema = self.ema10_history[1]
                self.log.info(f"Previous EMA(10): {previous_ema:.5f}", color=LogColor.BLUE)
        else:
            # During initialization phase, we still log the incoming data
            # but note that we're waiting for enough data to calculate the EMA
            self.log.info(
                f"Bar #{self.bars_processed} | "
                f"Close: {bar.close} | "
                "Waiting for EMA initialization...",
                color=LogColor.RED,
            )

    def on_stop(self):
        # Record strategy end time
        self.end_time = dt.datetime.now()
        self.log.info(
            f"Strategy finished at: {self.end_time} | "
            f"Duration: {(self.end_time - self.start_time).total_seconds():.2f} seconds.",
        )

        # Report how many bars were processed
        self.log.info(f"Strategy stopped. Processed {self.bars_processed} bars.")
