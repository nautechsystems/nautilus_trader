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

from dataclasses import dataclass

from nautilus_trader.common.enums import LogColor
from nautilus_trader.config import StrategyConfig
from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.message import Event
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.instruments import Instrument
from nautilus_trader.trading.strategy import Strategy


@dataclass
class Each10thBarEvent(Event):
    """
    A custom event that is published every 10th bar.

    By inheriting from `Event` class, we automatically get important attributes:
     - `id`: A unique string identifier for each event (in UUID format)
     - `ts_event`: Timestamp when the event occurred (used for event ordering)
     - `ts_init`: Timestamp when the event was initialized

    These attributes are crucial for correct event processing and ordering in the message bus,
    especially during backtesting where event timing is important.

    Event class offers complete flexibility in terms of attributes:
    - Can contain attributes of any Python type (int, float, str, custom objects, etc.)

    """

    bar: Bar  # The 10th bar related to this event
    TOPIC: str = "each_10th_bar_event"  # Topic name for message bus publish/subscribe


class DemoStrategyConfig(StrategyConfig, frozen=True):
    """
    Configuration for the demo strategy.
    """

    instrument: Instrument
    bar_type: BarType


class DemoStrategy(Strategy):
    """
    A demonstration strategy showing how to use custom events and the message bus.
    """

    def __init__(self, config: DemoStrategyConfig):
        super().__init__(config)

        # Counter for processed bars
        self.bars_processed = 0

    def on_start(self):
        # Subscribe to market data
        self.subscribe_bars(self.config.bar_type)
        self.log.info(f"Subscribed to {self.config.bar_type}", color=LogColor.YELLOW)

        # The message bus implements a topic-based publish/subscribe pattern:
        # - Publishers can publish events to one or more named topics
        # - Subscribers can subscribe to one or more topics of interest

        # Subscribe to our custom event
        # First argument is the topic name to subscribe to, second is the custom handler method
        self.msgbus.subscribe(Each10thBarEvent.TOPIC, self.on_each_10th_bar)
        self.log.info(f"Subscribed to {Each10thBarEvent.TOPIC}", color=LogColor.YELLOW)

    def on_bar(self, bar: Bar):
        # Count processed bars
        self.bars_processed += 1
        self.log.info(
            f"Bar #{self.bars_processed} | "
            f"Bar: {bar} | "
            f"Time={unix_nanos_to_dt(bar.ts_event)}",
        )

        # Every 10th bar, publish our custom event
        if self.bars_processed % 10 == 0:
            # Log our plans
            self.log.info(
                f"Going to publish event for topic: {Each10thBarEvent.TOPIC}",
                color=LogColor.GREEN,
            )

            # Create and publish the event
            # This demonstrates how to use the message bus to send events
            event = Each10thBarEvent(bar=bar)
            self.msgbus.publish(Each10thBarEvent.TOPIC, event)

    def on_each_10th_bar(self, event: Each10thBarEvent):
        """
        Handle each 10th bar event received from the message bus.
        """
        # Log the event details
        self.log.info(
            f"Received event for topic: {Each10thBarEvent.TOPIC} at bar # {self.bars_processed}| "
            f"Bar detail: {event.bar}",
            color=LogColor.RED,
        )

    def on_stop(self):
        self.log.info(f"Strategy stopped. Processed {self.bars_processed} bars.")
