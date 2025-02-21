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

from nautilus_trader.common.enums import LogColor
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.trading.strategy import Strategy


class DemoStrategy(Strategy):

    def __init__(self, bar_type_1min: BarType):
        super().__init__()

        # Extract the trading instrument's ID from the 1-minute bar configuration
        self.instrument_id = bar_type_1min.instrument_id

        # Save the 1-minute bar configuration and create a counter to track how many bars we receive
        self.bar_type_1min = bar_type_1min
        self.count_1min_bars = 0  # This will increment each time we receive a 1-minute bar

        # ==================================================================
        # POINT OF FOCUS: Creating a 5-minute bar configuration
        #
        # The BarType string has this format:
        # "{instrument_id}-{step}-{aggregation}-{price_type}-{source}"
        #
        # For example: "6EH4-5-MINUTE-LAST-INTERNAL" means:
        # - instrument_id: 6EH4 (the trading instrument)
        # - step: 5
        # - aggregation: MINUTE
        # - price_type: LAST (using last traded price)
        # - source of data: INTERNAL (aggregated by Nautilus) | EXTERNAL
        # ------------------------------------------------------------------

        # Aggregated 5-min bar data
        self.bar_type_5min = BarType.from_str(f"{self.instrument_id}-5-MINUTE-LAST-INTERNAL")
        self.count_5min_bars = 0  # Counter for received 5-minute bars

        # Track when the strategy starts and ends
        self.start_time = None
        self.end_time = None

    def on_start(self):
        # Save the exact time when strategy begins
        self.start_time = dt.datetime.now()
        self.log.info(f"Strategy started at: {self.start_time}")

        # Start receiving 1-minute bar updates
        self.subscribe_bars(self.bar_type_1min)

        # ==================================================================
        # POINT OF FOCUS: Setting up 5-minute bar aggregation
        #
        # To create 5-minute bars from 1-minute data, we need a special subscription format:
        # "{target_bar_type}@{source_bar_type}"
        #
        # The '@' symbol separates:
        # - Left side (target): what we want to create (5-minute bars)
        # - Right side (source): what data to use (1-minute external bars)
        #
        # Full format:
        # "{instrument_id}-{interval}-{unit}-{price_type}@{source_interval}-{source_unit}-{source}"
        #
        # Note: instrument_id and price_type are only needed in the left (target) part
        # ------------------------------------------------------------------

        # Start receiving 5-minute bar updates (created from 1-minute external data)
        bar_type_5min_subscribe = BarType.from_str(f"{self.bar_type_5min}@1-MINUTE-EXTERNAL")
        self.subscribe_bars(bar_type_5min_subscribe)

        # The on_bar() method below will handle all bars (both 1-minute and 5-minute bar updates).

    def on_bar(self, bar: Bar):
        # Process each bar based on its type
        match bar.bar_type:
            case self.bar_type_1min:  # if 1-minute bar is handled
                self.count_1min_bars += 1
                self.log.info(
                    f"1min bar detected: {bar} | Bar time: {unix_nanos_to_dt(bar.ts_event)}",
                    color=LogColor.GREEN,
                )
            case self.bar_type_5min:  # if 5-minute bar is handled
                self.count_5min_bars += 1
                self.log.info(
                    f"5min bar detected: {bar} | Bar time: {unix_nanos_to_dt(bar.ts_event)}",
                    color=LogColor.MAGENTA,
                )
            case _:
                raise Exception(f"Bar type not expected: {bar.bar_type}")

    def on_stop(self):
        # Save the exact time when strategy ends
        self.end_time = dt.datetime.now()
        self.log.info(f"Strategy finished at: {self.end_time}")

        # Show summary of how many bars we processed
        self.log.info(f"Total count of 1-MINUTE bars: {self.count_1min_bars}")
        self.log.info(f"Total count of 5-MINUTE bars: {self.count_5min_bars}")
