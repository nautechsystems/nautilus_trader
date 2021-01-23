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

from nautilus_trader.core.correctness import PyCondition
from tests.test_kit.performance import PerformanceHarness


class CorrectnessTests:

    @staticmethod
    def none():
        PyCondition.none(None, "param")

    @staticmethod
    def true():
        PyCondition.true(True, "this should be true")

    @staticmethod
    def valid_string():
        PyCondition.valid_string("abc123", "string_param")


class CorrectnessConditionPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_condition_none():
        PerformanceHarness.profile_function(CorrectnessTests.none, 100000, 1)
        # ~0.0ms / ~0.2μs / 200ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_condition_true():
        PerformanceHarness.profile_function(CorrectnessTests.true, 1, 100000)
        # ~0.0ms / ~0.2μs / 166ns minimum of 100,000 runs @ 1 iteration each run.
        # ~9.8ms / ~9763.3μs / 9763294ns minimum of 1 runs @ 100,000 iterations each run.

        # 100000 iterations @ 12ms with boolean except returning False
        # 100000 iterations @ 12ms with void except returning * !

    @staticmethod
    def test_condition_valid_string():
        PerformanceHarness.profile_function(CorrectnessTests.valid_string, 100000, 1)
        # ~0.0ms / ~0.2μs / 224ns minimum of 100,000 runs @ 1 iteration each run.
