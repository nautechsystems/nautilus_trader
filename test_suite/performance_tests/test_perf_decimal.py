# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_decimal.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import decimal
import unittest

from nautilus_trader.core.decimal import Decimal
from nautilus_trader.model.objects import Price

from test_kit.performance import PerformanceProfiler


_PRECISION_5_CONTEXT = decimal.Context(prec=5)
_STOCK_DECIMAL1 = decimal.Decimal('1.00000')
_STOCK_DECIMAL2 = decimal.Decimal('1.00001')

_DECIMAL1 = Decimal(1.00000, 5)
_DECIMAL2 = Decimal(1.00001, 5)


class PriceInitializations:

    @staticmethod
    def make_stock_decimal():
        decimal.Decimal('1.23456')

    @staticmethod
    def make_decimal():
        Decimal(1.23456, 5)

    @staticmethod
    def float_comparisons():
        # x1 = 1.0 > 2.0
        # x2 = 1.0 >= 2.0
        x3 = 1.0 == 2.0

    @staticmethod
    def float_arithmetic():
        x1 = 1.0 * 2.0

    @staticmethod
    def decimal_comparisons():
        # x1 = _DECIMAL1.value > _DECIMAL2.value
        # x2 = _DECIMAL1.value >= _DECIMAL2.value
        # x3 = _DECIMAL1.value == _DECIMAL2.value

        # x1 = _DECIMAL1.gt(_DECIMAL2)
        # x2 = _DECIMAL1.ge(_DECIMAL2)
        x3 = _DECIMAL1.eq(_DECIMAL1)

        # x3 = _DECIMAL1 == 1.0
        # x3 = _DECIMAL1.eq_float(1.0)

    @staticmethod
    def decimal_arithmetic():
        # x0 = _DECIMAL1 + 1.0
        x1 = _DECIMAL1 * 1.0

    @staticmethod
    def stock_decimal_comparisons():
        x1 = _STOCK_DECIMAL1 > _STOCK_DECIMAL2
        x2 = _STOCK_DECIMAL1 >= _STOCK_DECIMAL2
        x3 = _STOCK_DECIMAL1 == _STOCK_DECIMAL2

    @staticmethod
    def make_price():
        Price(1.23456, 5)

    @staticmethod
    def make_price_from_string():
        Price.from_string('1.23456')


class PricePerformanceTests(unittest.TestCase):

    @staticmethod
    def test_decimal_to_string():

        PerformanceProfiler.profile_function(_DECIMAL1.to_string, 1000000, 3)
        # ~417ms (416678μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_make_stock_decimal():

        PerformanceProfiler.profile_function(PriceInitializations.make_stock_decimal, 1000000, 3)
        # ~248ms (247206μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_make_decimal():

        PerformanceProfiler.profile_function(PriceInitializations.make_decimal, 1000000, 3)
        # ~174ms (173409μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_make_price():

        PerformanceProfiler.profile_function(PriceInitializations.make_price, 1000000, 3)
        # ~343ms (342190μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_float_comparisons():

        PerformanceProfiler.profile_function(PriceInitializations.float_comparisons, 1000000, 3)
        # ~61ms (60721μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_decimal_comparisons():

        PerformanceProfiler.profile_function(PriceInitializations.decimal_comparisons, 1000000, 3)
        # ~125ms (124726μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_stock_decimal_comparisons():

        PerformanceProfiler.profile_function(PriceInitializations.stock_decimal_comparisons, 1000000, 3)
        # ~166ms (165420μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_float_arithmetic():

        PerformanceProfiler.profile_function(PriceInitializations.float_arithmetic, 1000000, 3)
        # ~62ms (61914μs) average over 3 runs @ 1000000 iterations

    @staticmethod
    def test_decimal_arithmetic():

        PerformanceProfiler.profile_function(PriceInitializations.decimal_arithmetic, 1000000, 3)
        # ~212ms (211537μs) average over 3 runs @ 1000000 iterations
