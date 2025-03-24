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

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.model import InstrumentId
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import DataType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


class Last10BarsStats(Data):
    """
    A data class for storing statistics of the last 10 bars.

    This class inherits from Data class which is required for using Actor/Strategy publish/subscribe methods.
    The Data class inheritance automatically provides:
    - `ts_event` attribute: Used for proper data ordering in backtests
    - `ts_init` attribute: Used for initialization time tracking

    Since this class doesn't use @customdataclass decorator, it can have attributes of any Python types
    (like the complex BarType attribute). This is suitable for strategies where data
    doesn't need to be serialized, transferred between nodes or persisted.

    """

    bar_type: BarType
    last_bar_index: int = 0
    max_price: float = 0.0
    min_price: float = 0.0
    volume_total: float = 0.0


# Just an example of a serializable data class, we don't use it in this strategy
@customdataclass
class Last10BarsStatsSerializable(Data):
    """
    A serializable data class for storing statistics of the last 10 bars.

    This class uses the @customdataclass decorator which adds serialization capabilities
    required for:
    - Data persistence in the catalog system
    - Data transfer between different nodes
    - Automatic serialization methods: to_dict(), from_dict(), to_bytes(), to_arrow()

    Note: When using @customdataclass, attributes must be of supported types only:
    - InstrumentId
    - Basic types: str, bool, float, int, bytes, ndarray

    This example demonstrates proper usage of @customdataclass, though in this simple
    strategy we use `Last10BarsStats` instead as we don't need serialization capabilities.

    """

    instrument_id: InstrumentId
    last_bar_index: int = 0
    max_price: float = 0.0
    min_price: float = 0.0
    volume_total: float = 0.0


class DemoStrategyConfig(StrategyConfig, frozen=True):
    instrument: Instrument
    bar_type: BarType


class DemoStrategy(Strategy):
    """
    A demonstration strategy showing how to publish and subscribe to custom data.
    """

    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config)

        # Counter for processed bars
        self.bars_processed = 0

    def on_start(self):
        # Subscribe to market data
        self.subscribe_bars(self.config.bar_type)
        self.log.info(f"Subscribed to {self.config.bar_type}", color=LogColor.YELLOW)

        # Subscribe to our custom data type
        self.subscribe_data(DataType(Last10BarsStats))
        self.log.info("Subscribed to data of type: Last10BarsStatistics", color=LogColor.YELLOW)

    def on_bar(self, bar: Bar):
        # Count processed bars
        self.bars_processed += 1
        self.log.info(
            f"Bar #{self.bars_processed} | "
            f"Bar: {bar} | "
            f"Time={unix_nanos_to_dt(bar.ts_event)}",
        )

        # Every 10th bar, publish our custom data
        if self.bars_processed % 10 == 0:
            # Log our plans
            self.log.info(
                "Going to publish data of type: Last10BarsStatistics",
                color=LogColor.GREEN,
            )

            # Get the last 10 bars from the cache
            last_10_bars_list = self.cache.bars(self.config.bar_type)[:10]
            # Create data object
            data = Last10BarsStats(
                bar_type=bar.bar_type,
                last_bar_index=self.bars_processed - 1,
                max_price=max(bar.high for bar in last_10_bars_list),
                min_price=min(bar.low for bar in last_10_bars_list),
                volume_total=sum(bar.volume for bar in last_10_bars_list),
                ts_event=bar.ts_event,  # This field was added by @customdataclass decorator
                ts_init=bar.ts_init,  # This field was added by @customdataclass decorator
            )
            # Publish the data
            self.publish_data(DataType(Last10BarsStats), data)

    def on_data(self, data: Data):
        """
        Process received data from subscribed data sources.
        """
        if isinstance(data, Last10BarsStats):
            self.log.info(
                f"Received Last10BarsStatistics data: {data}",
                color=LogColor.RED,
            )

    def on_stop(self):
        self.log.info(f"Strategy stopped. Processed {self.bars_processed} bars.")
