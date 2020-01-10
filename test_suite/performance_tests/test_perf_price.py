# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_price.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import decimal
import unittest
import timeit

from time import time

from nautilus_trader.model.objects import Decimal, Price

MILLISECONDS_IN_SECOND = 1000
PRECISION_5_CONTEXT = decimal.Context(prec=5)

_DECIMAL1 = Decimal(1.00000, 5)
_DECIMAL2 = Decimal(1.00001, 5)

_STOCK_DECIMAL1 = decimal.Decimal('1.00000')
_STOCK_DECIMAL2 = decimal.Decimal('1.00001')


class PriceInitializations:

    @staticmethod
    def make_stock_decimal():
        decimal.Decimal(1.23456, PRECISION_5_CONTEXT)

    @staticmethod
    def make_decimal():
        Decimal(1.23456, 5)

    @staticmethod
    def float_comparisons():
        x1 = 1.0 > 2.0
        x2 = 1.0 >= 2.0
        x3 = 1.0 == 2.0

    @staticmethod
    def decimal_comparisons():
        x1 = _DECIMAL1.value > _DECIMAL2.value
        x2 = _DECIMAL1.value >= _DECIMAL2.value
        x3 = _DECIMAL1.value == _DECIMAL2.value

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
    def test_float_comparisons():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.float_comparisons).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~58ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_decimal_comparisons():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.decimal_comparisons).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~129ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_stock_decimal_comparisons():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.stock_decimal_comparisons).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~129ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_make_stock_decimal():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.make_stock_decimal).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~393ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_make_decimal():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.make_experimental_decimal).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~439ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_make_decimal():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.make_decimal).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_decimal_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~685ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_make_price():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.make_price).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_price_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~665ms for 1000000 prices
