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

import decimal
import unittest

from nautilus_trader.model.objects import Decimal
from nautilus_trader.model.objects import Price
from tests.test_kit.performance import PerformanceHarness

_PRECISION_5_CONTEXT = decimal.Context(prec=5)
_BUILTIN_DECIMAL1 = decimal.Decimal("1.00000")
_BUILTIN_DECIMAL2 = decimal.Decimal("1.00001")

_DECIMAL1 = Decimal("1.00000")
_DECIMAL2 = Decimal("1.00001")


class DecimalTesting:

    @staticmethod
    def make_builtin_decimal():
        decimal.Decimal("1.23456")

    @staticmethod
    def make_decimal():
        Decimal("1.23456")

    @staticmethod
    def float_comparisons():
        1.0 == 2.0  # noqa

    @staticmethod
    def float_arithmetic():
        1.0 * 2.0  # noqa

    @staticmethod
    def decimal_arithmetic():
        _DECIMAL1 + _DECIMAL1  # noqa
        _DECIMAL1 - _DECIMAL1  # noqa
        _DECIMAL1 * _DECIMAL1  # noqa
        _DECIMAL1 / _DECIMAL1  # noqa

    @staticmethod
    def decimal_arithmetic_with_floats():
        _DECIMAL1 + 1.0  # noqa
        _DECIMAL1 - 1.0  # noqa
        _DECIMAL1 * 1.0  # noqa
        _DECIMAL1 / 1.0  # noqa

    @staticmethod
    def builtin_decimal_arithmetic():
        _BUILTIN_DECIMAL1 + _BUILTIN_DECIMAL1  # noqa
        _BUILTIN_DECIMAL1 - _BUILTIN_DECIMAL1  # noqa
        _BUILTIN_DECIMAL1 * _BUILTIN_DECIMAL1  # noqa
        _BUILTIN_DECIMAL1 / _BUILTIN_DECIMAL1  # noqa

    @staticmethod
    def decimal_comparisons():
        _DECIMAL1 > _DECIMAL2  # noqa
        _DECIMAL1 >= _DECIMAL2  # noqa
        _DECIMAL1 == _DECIMAL2  # noqa

    @staticmethod
    def builtin_decimal_comparisons():
        _BUILTIN_DECIMAL1 > _BUILTIN_DECIMAL2  # noqa
        _BUILTIN_DECIMAL1 >= _BUILTIN_DECIMAL2  # noqa
        _BUILTIN_DECIMAL1 == _BUILTIN_DECIMAL2  # noqa

    @staticmethod
    def make_price():
        Price("1.23456")

    @staticmethod
    def make_price_from_float():
        Price.from_float(1.23456, 5)


class DecimalPerformanceTests(unittest.TestCase):

    def test_builtin_decimal_size(self):
        result = PerformanceHarness.object_size(decimal.Decimal("1.00000"))
        # Object size test: <class 'nautilus_trader.base.decimal.Decimal'> is 104 bytes
        self.assertTrue(result == 104)

    def test_decimal_size(self):
        result = PerformanceHarness.object_size(_DECIMAL1)
        # Object size test: <class 'nautilus_trader.model.objects.Decimal'> is 116 bytes.
        self.assertTrue(result <= 116)

    def test_make_builtin_decimal(self):
        result = PerformanceHarness.profile_function(DecimalTesting.make_builtin_decimal, 3, 100000)
        # ~23ms (23412μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.3)

    def test_make_decimal(self):
        result = PerformanceHarness.profile_function(DecimalTesting.make_decimal, 3, 100000)
        # ~59ms (59761μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 1.5)

    def test_make_price(self):
        result = PerformanceHarness.profile_function(DecimalTesting.make_price, 3, 100000)
        # ~124ms (124959μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 2.0)

    def test_float_comparisons(self):
        result = PerformanceHarness.profile_function(DecimalTesting.float_comparisons, 3, 100000)
        # ~6ms (6314μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.3)

    def test_decimal_comparisons(self):
        result = PerformanceHarness.profile_function(DecimalTesting.decimal_comparisons, 3, 100000)
        # ~61ms (613183μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result <= 1.5)

    def test_builtin_decimal_comparisons(self):
        result = PerformanceHarness.profile_function(DecimalTesting.builtin_decimal_comparisons, 3, 100000)
        # ~15ms (159896μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.3)

    def test_float_arithmetic(self):
        result = PerformanceHarness.profile_function(DecimalTesting.float_arithmetic, 3, 100000)
        # ~4ms (48702μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.3)

    def test_builtin_decimal_arithmetic(self):
        result = PerformanceHarness.profile_function(DecimalTesting.builtin_decimal_arithmetic, 3, 100000)
        # ~47ms (477540μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 0.6)

    def test_decimal_arithmetic(self):
        result = PerformanceHarness.profile_function(DecimalTesting.decimal_arithmetic, 3, 100000)
        # ~138ms (1872823μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 2.0)

    def test_decimal_arithmetic_with_floats(self):
        result = PerformanceHarness.profile_function(DecimalTesting.decimal_arithmetic_with_floats, 3, 100000)
        # ~138ms (1872823μs) minimum of 3 runs @ 1,000,000 iterations each run.
        self.assertTrue(result < 2.0)
