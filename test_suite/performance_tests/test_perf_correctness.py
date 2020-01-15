# -------------------------------------------------------------------------------------------------
# <copyright file="test_perf_correctness.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.correctness import PyCondition
from test_kit.stubs import TestStubs
from test_kit.performance import PerformanceProfiler

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
        test_function = CorrectnessConditionPerformanceTest().true

        # Test
        PerformanceProfiler.profile_function(test_function, 3, 100000)
        # ~12ms (12012μs) minimum of 5 runs @ 100000 iterations

        # 100000 iterations @ 12ms with boolean except returning False
        # 100000 iterations @ 12ms with void except returning * !

    @staticmethod
    def test_condition_valid_string():
        # Arrange
        test_function = CorrectnessConditionPerformanceTest().valid_string

        # Test
        PerformanceProfiler.profile_function(test_function, 3, 100000)
        # ~15ms (15622μs) minimum of 5 runs @ 100000 iterations
