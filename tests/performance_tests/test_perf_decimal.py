# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.inspect import get_size_of
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.performance import PerformanceBench
from nautilus_trader.test_kit.performance import PerformanceHarness


_PRECISION_5_CONTEXT = decimal.Context(prec=5)
_BUILTIN_DECIMAL1 = Decimal("1.00000")
_BUILTIN_DECIMAL2 = Decimal("1.00001")

_DECIMAL1 = Quantity(1, precision=1)
_DECIMAL2 = Quantity(1.00001, precision=5)


class DecimalTesting:
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


class TestDecimalPerformance(PerformanceHarness):
    def test_builtin_decimal_size(self):
        print(get_size_of(Decimal("1.00000")))
        # Object size <class 'decimal.Decimal'> is 104 bytes.

    def test_decimal_size(self):
        print(get_size_of(_DECIMAL1))
        # Object size <class 'nautilus_trader.model.objects.BaseDecimal'> is 152 bytes.

    def test_make_builtin_decimal(self):
        self.benchmark.pedantic(
            target=Decimal,
            args=("1.23456",),
            iterations=1,
            rounds=100_000,
        )
        # ~0.0ms / ~0.3μs / 253ns minimum of 100,000 runs @ 1 iteration each run.

    def test_make_decimal(self):
        self.benchmark.pedantic(
            target=Quantity,
            args=(1.23456, 5),
            iterations=1,
            rounds=100_000,
        )
        # ~0.0ms / ~0.4μs / 353ns minimum of 100,000 runs @ 1 iteration each run.

    def test_make_price(self):
        # self.benchmark.pedantic(
        #     target=Price,
        #     args=(1.23456, 5),
        #     iterations=1,
        #     rounds=100_000,
        # )
        def make_price():
            Price(1.23456, 5)

        PerformanceBench.profile_function(
            target=make_price,
            runs=100_000,
            iterations=1,
        )
        # ~0.0ms / ~0.5μs / 526ns minimum of 100,000 runs @ 1 iteration each run.
        # ~0.0ms / ~0.2μs / 193ns minimum of 100,000 runs @ 1 iteration each run.

    def test_make_price_from_float(self):
        self.benchmark.pedantic(
            target=Price,
            args=(1.23456, 5),
            iterations=1,
            rounds=100_000,
        )

    def test_float_comparisons(self):
        self.benchmark.pedantic(
            target=DecimalTesting.float_comparisons,
            iterations=1,
            rounds=100_000,
        )
        # ~0.0ms / ~0.1μs / 118ns minimum of 100,000 runs @ 1 iteration each run.

    def test_decimal_comparisons(self):
        self.benchmark.pedantic(
            target=DecimalTesting.decimal_comparisons,
            iterations=1,
            rounds=100_000,
        )
        # ~0.0ms / ~0.4μs / 429ns minimum of 100,000 runs @ 1 iteration each run.

    def test_builtin_decimal_comparisons(self):
        self.benchmark.pedantic(
            target=DecimalTesting.builtin_decimal_comparisons,
            iterations=1,
            rounds=100_000,
        )
        # ~17.2ms / ~17237.6μs / 17237551ns minimum of 3 runs @ 100,000 iterations each run.

    def test_float_arithmetic(self):
        self.benchmark.pedantic(
            target=DecimalTesting.float_arithmetic,
            iterations=1,
            rounds=100_000,
        )
        # ~5.0ms / ~5027.3μs / 5027301ns minimum of 3 runs @ 100,000 iterations each run.

    def test_builtin_decimal_arithmetic(self):
        self.benchmark.pedantic(
            target=DecimalTesting.builtin_decimal_arithmetic,
            iterations=1,
            rounds=100_000,
        )

        PerformanceBench.profile_function(
            target=DecimalTesting.builtin_decimal_arithmetic,
            runs=100_000,
            iterations=1,
        )
        # ~0.0ms / ~0.4μs / 424ns minimum of 100,000 runs @ 1 iteration each run.

    def test_decimal_arithmetic(self):
        self.benchmark.pedantic(
            target=DecimalTesting.decimal_arithmetic,
            iterations=1,
            rounds=100_000,
        )
        # ~71.0ms / ~70980.9μs / 70980863ns minimum of 3 runs @ 100,000 iterations each run.

    def test_decimal_arithmetic_with_floats(self):
        self.benchmark.pedantic(
            target=DecimalTesting.decimal_arithmetic_with_floats,
            iterations=1,
            rounds=100_000,
        )
        # ~58.0ms / ~58034.9μs / 58034884ns minimum of 3 runs @ 100,000 iterations each run.
