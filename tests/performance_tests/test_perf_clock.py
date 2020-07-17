# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
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

import unittest
from datetime import timedelta

from nautilus_trader.common.logging import LogLevel, LoggerAdapter, TestLogger
from nautilus_trader.common.clock import TestClock
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import UNIX_EPOCH

clock = TestClock()


class TestClockTests:

    @staticmethod
    def advance_time():
        test_time = UNIX_EPOCH
        for i in range(1000000):
            test_time += timedelta(seconds=1)
        clock.advance_time(test_time)


class TestClockPerformanceTests(unittest.TestCase):

    def test_advance_time(self):
        logger = LoggerAdapter('TestClock', TestLogger(level_console=LogLevel.DEBUG))
        store = []
        clock.register_logger(logger)
        clock.set_timer('test', timedelta(seconds=1), handler=store.append)

        iterations = 1
        result = PerformanceHarness.profile_function(TestClockTests.advance_time, 1, iterations)
        # ~1484ms (1484100μs) minimum of 1 runs @ 1000000 iterations each run.
        self.assertTrue(result < 2.0)
