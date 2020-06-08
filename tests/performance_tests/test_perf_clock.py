# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import timedelta

from nautilus_trader.core.types import Label
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
        clock.set_timer(Label('test'), timedelta(seconds=1), handler=store.append)

        iterations = 1
        result = PerformanceHarness.profile_function(TestClockTests.advance_time, 1, iterations)
        # ~1036ms (1036473μs) minimum of 1 runs @ 1000000 iterations each run.
        self.assertTrue(result < 1.5)
