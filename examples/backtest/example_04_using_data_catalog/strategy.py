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

        # Track when the strategy starts and ends
        self.start_time = None
        self.end_time = None

    def on_start(self):
        # Save the exact time when strategy begins
        self.start_time = dt.datetime.now()
        self.log.info(f"Strategy started at: {self.start_time}")

        # Start receiving 1-minute bar updates
        self.subscribe_bars(self.bar_type_1min)

    def on_bar(self, bar: Bar):
        # You can implement any action here (like submit order), but for simplicity, we are just counting bars
        self.count_1min_bars += 1
        self.log.info(
            f"Bar #{self.count_1min_bars} | Time: {unix_nanos_to_dt(bar.ts_event):%Y-%m-%d %H:%M:%S} | Bar: {bar}",
            color=LogColor.BLUE,
        )

    def on_stop(self):
        # Save the exact time when strategy ends
        self.end_time = dt.datetime.now()
        self.log.info(f"Strategy finished at: {self.end_time}")

        # Show summary of how many bars we processed
        self.log.info(f"Total count of 1-MINUTE bars: {self.count_1min_bars}")
