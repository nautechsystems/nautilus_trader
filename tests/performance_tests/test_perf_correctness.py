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

from nautilus_trader.core.correctness import PyCondition
from tests.test_kit.stubs import TestStubs
from tests.test_kit.performance import PerformanceHarness

USDJPY_FXCM = TestStubs.instrument_usdjpy()


class CorrectnessTests:

    @staticmethod
    def none():
        PyCondition.none(None, 'param')

    @staticmethod
    def true():
        PyCondition.true(True, 'this should be true')

    @staticmethod
    def valid_string():
        PyCondition.valid_string('abc123', 'string_param')


class CorrectnessConditionPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_condition_true():
        # Test
        PerformanceHarness.profile_function(CorrectnessTests.none, 3, 100000)
        # ~11ms (11827μs) minimum of 3 runs @ 100,000 iterations each run

    @staticmethod
    def test_condition_not_none():
        # Test
        PerformanceHarness.profile_function(CorrectnessTests.true, 3, 100000)
        # ~12ms (12012μs) minimum of 5 runs @ 100000 iterations

        # 100000 iterations @ 12ms with boolean except returning False
        # 100000 iterations @ 12ms with void except returning * !

    @staticmethod
    def test_condition_valid_string():
        # Test
        PerformanceHarness.profile_function(CorrectnessTests.valid_string, 3, 100000)
        # ~15ms (15622μs) minimum of 5 runs @ 100000 iterations
