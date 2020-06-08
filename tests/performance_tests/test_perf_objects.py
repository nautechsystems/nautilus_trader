# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.objects import Price, Volume, Bar

from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import TestStubs, UNIX_EPOCH


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
                  Volume(100000),
                  UNIX_EPOCH,
                  check=False)

    @staticmethod
    def build_bar_with_checking():
        bar = Bar(Price(1.00001, 5),
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  Volume(100000),
                  UNIX_EPOCH,
                  check=True)


class ObjectPerformanceTests(unittest.TestCase):

    def test_symbol_using_str(self):
        result = PerformanceHarness.profile_function(ObjectTests.symbol_using_str, 3, 1000000)
        # ~140ms (140233μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_symbol_using_to_string(self):
        result = PerformanceHarness.profile_function(ObjectTests.symbol_using_to_string, 3, 1000000)
        # ~103ms (103260μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_build_bar_no_checking(self):
        result = PerformanceHarness.profile_function(ObjectTests.build_bar_no_checking, 3, 100000)
        # ~146ms (146283μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.2)

    def test_build_bar_with_checking(self):
        result = PerformanceHarness.profile_function(ObjectTests.build_bar_with_checking, 3, 100000)
        # ~143ms (143914μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.2)
