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

import numpy as np

from nautilus_trader.core.functions import fast_mean
from nautilus_trader.core.functions import fast_std
from tests.test_kit.performance import PerformanceHarness


class FunctionPerformanceTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.values = list(np.random.rand(10))

    def np_mean(self):
        np.mean(self.values)

    def np_std(self):
        np.std(self.values)

    def fast_mean(self):
        fast_mean(self.values)

    def fast_std(self):
        fast_std(self.values)

    def test_np_mean(self):
        PerformanceHarness.profile_function(self.np_mean, 10000, 1)
        # ~0ms / ~9μs / 8464ns minimum of 10000 runs @ 1 iterations each run.

    def test_np_std(self):
        PerformanceHarness.profile_function(self.np_std, 10000, 1)
        # ~0ms / ~20μs / 19517ns minimum of 10000 runs @ 1 iterations each run.

    def test_fast_mean(self):
        PerformanceHarness.profile_function(self.fast_mean, 100000, 1)
        # ~0ms / ~1μs / 945ns minimum of 100000 runs @ 1 iterations each run.

    def test_fast_std(self):
        PerformanceHarness.profile_function(self.fast_std, 100000, 1)
        # ~0ms / ~3μs / 2015ns minimum of 100000 runs @ 1 iterations each run.
