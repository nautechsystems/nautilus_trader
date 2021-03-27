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
from tests.test_kit.performance import PerformanceHarness


class TestThrottlerPerformance:
    def setup(self):
        # Fixture setup
        self.clock = TestClock()
        self.logger = TestLogger(self.clock, bypass_logging=True)

        self.handler = []
        self.throttler = Throttler(
            name="Throttler-1",
            limit=10000,
            interval=timedelta(seconds=1),
            output=self.handler.append,
            clock=self.clock,
            logger=self.logger,
        )

    def send(self):
        self.throttler.send("MESSAGE")

    def test_send_unlimited(self):
        PerformanceHarness.profile_function(self.send, 10000, 1)
        # ~0.0ms / ~0.3μs / 301ns minimum of 10,000 runs @ 1 iteration each run.

    def test_send_when_limited(self):
        PerformanceHarness.profile_function(self.send, 100000, 1)
        # ~0.0ms / ~0.2μs / 232ns minimum of 100,000 runs @ 1 iteration each run.
