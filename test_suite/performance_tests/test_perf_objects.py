# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_objects.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.objects import Price, Bar

from test_kit.performance import PerformanceProfiler
from test_kit.stubs import TestStubs, UNIX_EPOCH


AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
AUDUSD_1MIN_BID = TestStubs.bartype_audusd_1min_bid()


class ObjectTests:

    @staticmethod
    def symbol_using_str():
        str(AUDUSD_FXCM)

    @staticmethod
    def symbol_using_to_string():
        AUDUSD_FXCM.to_string()

    @staticmethod
    def build_bar_no_checking():
        bar = Bar(Price(1.00001, 5),
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  100000,
                  UNIX_EPOCH,
                  check=False)

    @staticmethod
    def build_bar_with_checking():
        bar = Bar(Price(1.00001, 5),
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  100000,
                  UNIX_EPOCH,
                  check=True)


class ObjectPerformanceTests(unittest.TestCase):

    def test_symbol_using_str(self):
        result = PerformanceProfiler.profile_function(ObjectTests.symbol_using_str, 3, 1000000)
        # ~138ms (138291μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_symbol_using_to_string(self):
        result = PerformanceProfiler.profile_function(ObjectTests.symbol_using_to_string, 3, 1000000)
        # ~90ms (90342μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_build_bar_no_checking(self):
        result = PerformanceProfiler.profile_function(ObjectTests.build_bar_no_checking, 3, 100000)
        # ~113ms (113953μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_build_bar_with_checking(self):
        result = PerformanceProfiler.profile_function(ObjectTests.build_bar_with_checking, 3, 100000)
        # ~117ms (117651μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.2)
