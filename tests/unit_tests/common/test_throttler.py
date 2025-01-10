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

from datetime import timedelta

from nautilus_trader.common.component import TestClock
from nautilus_trader.common.component import Throttler


class TestBufferingThrottler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

        self.handler = []
        self.throttler = Throttler(
            name="Buffer",
            limit=5,
            interval=timedelta(seconds=1),
            output_send=self.handler.append,
            output_drop=None,  # <-- no dropping handler so will buffer
            clock=self.clock,
        )

    def test_throttler_instantiation(self):
        # Arrange, Act, Assert
        assert self.throttler.name == "Buffer"
        assert not self.throttler.is_limiting
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 0
        assert self.throttler.sent_count == 0

    def test_send_sends_message_to_handler(self):
        # Arrange
        item = "MESSAGE"

        # Act
        self.throttler.send(item)

        # Assert
        assert not self.throttler.is_limiting
        assert self.handler == ["MESSAGE"]
        assert self.throttler.recv_count == 1
        assert self.throttler.sent_count == 1

    def test_send_to_limit_becomes_throttled(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Assert: Only 5 items are sent
        assert self.clock.timer_names == ["Buffer|DEQUE"]
        assert self.clock.timer_count == 1
        assert self.throttler.is_limiting
        assert self.handler == ["MESSAGE"] * 5
        assert self.throttler.qsize == 1
        assert self.throttler.used() == 1
        assert self.throttler.recv_count == 6
        assert self.throttler.sent_count == 5

    def test_used_when_sent_to_limit_returns_one(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act
        used = self.throttler.used()

        # Assert: Remaining items sent
        assert used == 1
        assert self.throttler.recv_count == 5
        assert self.throttler.sent_count == 5

    def test_used_when_half_interval_from_limit_returns_half(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.clock.advance_time(500_000_000)

        # Act
        used = self.throttler.used()

        # Assert: Remaining items sent
        assert used == 0.5
        assert self.throttler.recv_count == 5
        assert self.throttler.sent_count == 5

    def test_used_before_limit_when_halfway_returns_half(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act
        used = self.throttler.used()

        # Assert
        assert used == 0.6

    def test_refresh_when_at_limit_sends_remaining_items(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act: Trigger refresh token time alert
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        # Assert: Remaining items sent
        assert self.clock.timer_count == 0  # No longer timing to process
        assert self.throttler.is_limiting is False
        assert self.handler == ["MESSAGE"] * 6
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 6
        assert self.throttler.sent_count == 6

    def test_send_message_after_dropping_message(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act: Trigger refresh token time alert
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        assert self.throttler.is_limiting is False

        # Act: send a message after a previous message is throttled
        self.throttler.send(item)

        # Assert: Remaining items sent
        assert self.clock.timer_count == 0  # No longer timing to process
        assert self.throttler.is_limiting is False
        assert self.handler == ["MESSAGE"] * 7
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 7
        assert self.throttler.sent_count == 7


class TestDroppingThrottler:
    def setup(self):
        # Fixture Setup
        self.clock = TestClock()

        self.handler = []
        self.dropped = []
        self.throttler = Throttler(
            name="Dropper",
            limit=5,
            interval=timedelta(seconds=1),
            output_send=self.handler.append,
            output_drop=self.dropped.append,  # <-- handler for dropping messages
            clock=self.clock,
        )

    def test_throttler_instantiation(self):
        # Arrange, Act, Assert
        assert self.throttler.name == "Dropper"
        assert not self.throttler.is_limiting
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 0
        assert self.throttler.sent_count == 0

    def test_send_sends_message_to_handler(self):
        # Arrange
        item = "MESSAGE"

        # Act
        self.throttler.send(item)

        # Assert
        assert not self.throttler.is_limiting
        assert self.handler == ["MESSAGE"]
        assert self.throttler.recv_count == 1
        assert self.throttler.sent_count == 1

    def test_send_to_limit_drops_message(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Assert: Only 5 items are sent
        assert self.clock.timer_names == ["Dropper|DEQUE"]
        assert self.clock.timer_count == 1
        assert self.throttler.is_limiting
        assert self.handler == ["MESSAGE"] * 5
        assert self.dropped == ["MESSAGE"]
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 1
        assert self.throttler.recv_count == 6
        assert self.throttler.sent_count == 5

    def test_advance_time_when_at_limit_dropped_message(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act: Trigger refresh token time alert
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        # Assert: Remaining items sent
        assert self.clock.timer_count == 0  # No longer timing to process
        assert self.throttler.is_limiting is False
        assert self.handler == ["MESSAGE"] * 5
        assert self.dropped == ["MESSAGE"]
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 6
        assert self.throttler.sent_count == 5

    def test_send_message_after_dropping_message(self):
        # Arrange
        item = "MESSAGE"

        # Act: Send 6 items
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)
        self.throttler.send(item)

        # Act: Trigger refresh token time alert
        events = self.clock.advance_time(1_000_000_000)
        events[0].handle()

        assert self.throttler.is_limiting is False

        # Act: send a message after a previous message is throttled
        self.throttler.send(item)

        # Assert: Remaining items sent
        assert self.clock.timer_count == 0  # No longer timing to process
        assert self.throttler.is_limiting is False
        assert self.handler == ["MESSAGE"] * 6
        assert self.dropped == ["MESSAGE"]
        assert self.throttler.qsize == 0
        assert self.throttler.used() == 0
        assert self.throttler.recv_count == 7
        assert self.throttler.sent_count == 6
