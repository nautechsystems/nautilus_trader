# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_clock.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from datetime import timedelta

from nautilus_trader.model.identifiers import Label
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.logger import LogLevel, LoggerAdapter, TestLogger
from test_kit.performance import PerformanceProfiler
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()

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
        result = PerformanceProfiler.profile_function(TestClockTests.advance_time, 1, iterations)
        # ~1036ms (1036473μs) minimum of 1 runs @ 1000000 iterations each run.
        self.assertTrue(result < 1.5)
