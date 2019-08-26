# -------------------------------------------------------------------------------------------------
# <copyright file="test_correctness_perf.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import math
import unittest
import time
import timeit

from nautilus_trader.core.correctness import PyCondition
from test_kit.stubs import TestStubs

MILLISECONDS_IN_SECOND = 1000
MICROSECONDS_IN_SECOND = 1000000
USDJPY_FXCM = TestStubs.instrument_usdjpy()


class CorrectnessConditionPerformanceTest:
    @staticmethod
    def true():

        PyCondition.true(True, 'this should be true')

    @staticmethod
    def valid_string():
        PyCondition.valid_string('abc123', 'string_param')


class CorrectnessConditionPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_condition_true():
        # Arrange
        test_cycles = 3
        test_iterations = 100000
        test_function = CorrectnessConditionPerformanceTest().true

        total_elapsed = 0

        for x in range(test_cycles):
            srt_time = time.time()
            for x in range(test_iterations):
                test_function()
            end_time = time.time()
            total_elapsed += end_time - srt_time

        print('\n' + f'performance test of {test_cycles} cycles @ {test_iterations} iterations')
        print(f'average elapsed = '
              f'{math.ceil((total_elapsed / test_cycles) * MICROSECONDS_IN_SECOND)}μs ('
              f'{math.ceil((total_elapsed / test_cycles) * MILLISECONDS_IN_SECOND)}ms)')

        # 100000 iterations @ 12ms with boolean except returning False
        # 100000 iterations @ 12ms with void except returning * !

    @staticmethod
    def test_condition_valid_string():
        # Arrange
        test_cycles = 3
        test_iterations = 100000
        test_function = CorrectnessConditionPerformanceTest().valid_string

        total_elapsed = 0

        for x in range(test_cycles):
            srt_time = time.time()
            for x in range(test_iterations):
                test_function()
            end_time = time.time()
            total_elapsed += end_time - srt_time

        print('\n' + f'performance test of {test_cycles} cycles @ {test_iterations} iterations')
        print(f'average elapsed = '
              f'{math.ceil((total_elapsed / test_cycles) * MICROSECONDS_IN_SECOND)}μs ('
              f'{math.ceil((total_elapsed / test_cycles) * MILLISECONDS_IN_SECOND)}ms)')

        # 100000 iterations @ 16ms
