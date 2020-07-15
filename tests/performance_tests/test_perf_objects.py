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
        bar = Bar(Price(1.00001, 5),  # noqa
                  Price(1.00004, 5),
                  Price(1.00002, 5),
                  Price(1.00003, 5),
                  Volume(100000),
                  UNIX_EPOCH,
                  check=False)

    @staticmethod
    def build_bar_with_checking():
        bar = Bar(Price(1.00001, 5),  # noqa
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
