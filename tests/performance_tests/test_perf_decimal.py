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

import decimal
from decimal import Decimal
import unittest

from nautilus_trader.model.objects import BaseDecimal
from nautilus_trader.model.objects import Price
from tests.test_kit.performance import PerformanceHarness


_PRECISION_5_CONTEXT = decimal.Context(prec=5)
_BUILTIN_DECIMAL1 = Decimal("1.00000")
_BUILTIN_DECIMAL2 = Decimal("1.00001")

_DECIMAL1 = BaseDecimal("1")
_DECIMAL2 = BaseDecimal("1.00001")


class DecimalTesting:

    @staticmethod
    def make_builtin_decimal():
        Decimal("1.23456")

    @staticmethod
    def make_decimal():
        BaseDecimal("1.23456")

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
        Price(1.23456, 5)


class DecimalPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_builtin_decimal_size():
        PerformanceHarness.object_size(Decimal("1.00000"))
        # Object size <class 'decimal.Decimal'> is 104 bytes.

    @staticmethod
    def test_decimal_size():
        PerformanceHarness.object_size(_DECIMAL1)
        # Object size <class 'nautilus_trader.model.objects.BaseDecimal'> is 168 bytes.

    @staticmethod
    def test_make_builtin_decimal():
        PerformanceHarness.profile_function(DecimalTesting.make_builtin_decimal, 100000, 1)
        # ~0.0ms / ~0.3μs / 253ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_make_decimal():
        PerformanceHarness.profile_function(DecimalTesting.make_decimal, 100000, 1)
        # ~0.0ms / ~0.3μs / 338ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_make_one_decimal():
        PerformanceHarness.profile_function(DecimalTesting.make_decimal, 100000, 1)
        # ~0.0ms / ~0.4μs / 359ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_make_price():
        PerformanceHarness.profile_function(DecimalTesting.make_price, 100000, 1)
        # ~0.0ms / ~0.5μs / 521ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_float_comparisons():
        PerformanceHarness.profile_function(DecimalTesting.float_comparisons, 100000, 1)
        # ~0.0ms / ~0.1μs / 122ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_decimal_comparisons():
        PerformanceHarness.profile_function(DecimalTesting.decimal_comparisons, 100000, 1)
        # ~0.0ms / ~0.4μs / 429ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_builtin_decimal_comparisons():
        PerformanceHarness.profile_function(DecimalTesting.builtin_decimal_comparisons, 3, 100000)
        # ~16ms (16410μs) minimum of 3 runs @ 100,000 iterations each run.

    @staticmethod
    def test_float_arithmetic():
        PerformanceHarness.profile_function(DecimalTesting.float_arithmetic, 3, 100000)
        # ~4ms (48702μs) minimum of 3 runs @ 1,000,000 iterations each run.

    @staticmethod
    def test_builtin_decimal_arithmetic():
        PerformanceHarness.profile_function(DecimalTesting.builtin_decimal_arithmetic, 3, 100000)
        # ~36ms (36215μs) minimum of 3 runs @ 100,000 iterations each run.

    @staticmethod
    def test_decimal_arithmetic():
        PerformanceHarness.profile_function(DecimalTesting.decimal_arithmetic, 3, 100000)
        # ~86ms (86808μs) minimum of 3 runs @ 100,000 iterations each run.

    @staticmethod
    def test_decimal_arithmetic_with_floats():
        PerformanceHarness.profile_function(DecimalTesting.decimal_arithmetic_with_floats, 3, 100000)
        # ~50ms (50253μs) minimum of 3 runs @ 100,000 iterations each run.
