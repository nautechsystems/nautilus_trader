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
import unittest

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.clock import TestClock
from tests.test_kit.performance import PerformanceHarness


live_clock = LiveClock()
test_clock = TestClock()


class LiveClockPerformanceTests(unittest.TestCase):
    @staticmethod
    def test_utc_now():
        PerformanceHarness.profile_function(live_clock.timestamp_ns, 100000, 1)
        # ~0.0ms / ~1.3μs / 1330ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_unix_timestamp():
        PerformanceHarness.profile_function(live_clock.timestamp, 100000, 1)
        # ~0.0ms / ~0.1μs / 101ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_unix_timestamp_ns():
        PerformanceHarness.profile_function(live_clock.timestamp_ns, 100000, 1)
        # ~0.0ms / ~0.1μs / 101ns minimum of 100,000 runs @ 1 iteration each run.


class TestClockHarness:
    @staticmethod
    def advance_time():
        test_clock.advance_time(to_time_ns=0)

    @staticmethod
    def iteratively_advance_time():
        test_time = 0
        for i in range(100000):
            test_time += 1
        test_clock.advance_time(to_time_ns=test_time)


class TestClockPerformanceTests(unittest.TestCase):
    @staticmethod
    def test_advance_time():
        PerformanceHarness.profile_function(TestClockHarness.advance_time, 100000, 1)
        # ~0.0ms / ~0.2μs / 175ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_iteratively_advance_time():
        store = []
        test_clock.set_timer("test", timedelta(seconds=1), handler=store.append)

        iterations = 1
        PerformanceHarness.profile_function(
            TestClockHarness.iteratively_advance_time, 1, iterations
        )
        # ~320.1ms                       minimum of 1 runs @ 1 iteration each run. (100000 advances)
        # ~3.7ms / ~3655.1μs / 3655108ns minimum of 1 runs @ 1 iteration each run.
