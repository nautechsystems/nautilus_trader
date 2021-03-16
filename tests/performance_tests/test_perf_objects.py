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

import unittest

from nautilus_trader.model.bar import Bar
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import TestStubs
from tests.test_kit.stubs import UNIX_EPOCH


AUDUSD_SIM = TestStubs.audusd_id()
AUDUSD_1MIN_BID = TestStubs.bartype_audusd_1min_bid()


class ObjectTests:
    @staticmethod
    def make_symbol():
        Symbol("AUD/USD")

    @staticmethod
    def make_instrument_id():
        InstrumentId(Symbol("AUD/USD"), Venue("IDEALPRO"))

    @staticmethod
    def instrument_id_to_str():
        str(AUDUSD_SIM)

    @staticmethod
    def build_bar_no_checking():
        Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity("100000"),
            UNIX_EPOCH,
            check=False,
        )

    @staticmethod
    def build_bar_with_checking():
        Bar(
            Price("1.00001"),
            Price("1.00004"),
            Price("1.00002"),
            Price("1.00003"),
            Quantity("100000"),
            UNIX_EPOCH,
            check=True,
        )


class ObjectPerformanceTests(unittest.TestCase):
    @staticmethod
    def test_make_symbol():
        PerformanceHarness.profile_function(ObjectTests.make_symbol, 100000, 1)
        # ~0.0ms / ~0.4μs / 400ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_make_instrument_id():
        PerformanceHarness.profile_function(ObjectTests.make_instrument_id, 100000, 1)
        # ~0.0ms / ~1.3μs / 1251ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_instrument_id_to_str():
        PerformanceHarness.profile_function(ObjectTests.instrument_id_to_str, 100000, 1)
        # ~0.0ms / ~0.2μs / 198ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_build_bar_no_checking():
        PerformanceHarness.profile_function(
            ObjectTests.build_bar_no_checking, 100000, 1
        )
        # ~0.0ms / ~2.5μs / 2512ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_build_bar_with_checking():
        PerformanceHarness.profile_function(
            ObjectTests.build_bar_with_checking, 100000, 1
        )
        # ~0.0ms / ~2.7μs / 2717ns minimum of 100,000 runs @ 1 iteration each run.
