#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="price_perf.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import timeit

from decimal import Decimal
from time import time

from inv_trader.model.objects import Price


MILLISECONDS_IN_SECOND = 1000


class PriceInitializations:

    @staticmethod
    def make_decimal():
        Decimal('1.00000')

    @staticmethod
    def from_string():
        Price('1.00000')

    @staticmethod
    def from_float():
        Price(1.00000, 5)

    @staticmethod
    def from_decimal():
        Price(Decimal('1.00000'))

    @staticmethod
    def from_string_add_pad():
        Price.from_string_add_pad('1.0000', 5)


class PricePerformanceTests(unittest.TestCase):

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

        # ~210ms for 1000000 decimals (wrapping adds 60ms)

    @staticmethod
    def test_price_from_string():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.from_string).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_price_from_string({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~385ms for 1000000 prices

    @staticmethod
    def test_price_from_float():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.from_float).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_price_from_float({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~850ms for 1000000 prices

    @staticmethod
    def test_price_from_decimal():
        # Arrange
        tests = 3
        number = 1000000

        total_elapsed = 0

        for x in range(tests):
            srt_time = time()
            timeit.Timer(PriceInitializations.from_decimal).timeit(number=number)
            end_time = time()
            total_elapsed += round((end_time - srt_time) * MILLISECONDS_IN_SECOND)

        print('\n' + f'test_price_from_decimal({number} iterations)')
        print(f'{round(total_elapsed / tests)}ms')

        # ~520ms for 1000000 prices
