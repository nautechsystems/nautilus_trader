# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.throttler import Throttler
from tests.test_kit.stubs import UNIX_EPOCH


class TestThrottler:

    def setup(self):
        # Fixture setup
        self.clock = TestClock()
        self.logger = TestLogger(self.clock)

        self.handler = []
        self.throttler = Throttler(
            name="Throttler-1",
            limit=5,
            interval=timedelta(seconds=1),
            output=self.handler.append,
            maxsize=10000,
            clock=self.clock,
            logger=self.logger,
        )

    def test_throttler_instantiation(self):
        # Arrange
        # Act
        # Assert
        assert "Throttler-1" == self.throttler.name
        assert 0 == self.throttler.qsize
        assert not self.throttler.is_active
        assert not self.throttler.is_throttling

    def test_send_when_not_active_becomes_active(self):
        # Arrange
        item = "MESSAGE"

        # Act
        self.throttler.send(item)

        # Assert
        assert self.throttler.is_active
        assert not self.throttler.is_throttling
        assert ["MESSAGE"] == self.handler

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
        assert ["Throttler-1-REFRESH-TOKEN"] == self.clock.timer_names()
        assert self.throttler.is_active
        assert self.throttler.is_throttling
        assert ["MESSAGE"] * 5 == self.handler
        assert 1 == self.throttler.qsize

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
        events = self.clock.advance_time(UNIX_EPOCH + timedelta(seconds=1))
        events[0].handle_py()

        # Assert: Remaining items sent
        assert self.clock.timer_names() == ["Throttler-1-REFRESH-TOKEN"]
        assert self.throttler.is_active
        assert self.throttler.is_throttling
        assert ["MESSAGE"] * 6 == self.handler
        assert 0 == self.throttler.qsize
