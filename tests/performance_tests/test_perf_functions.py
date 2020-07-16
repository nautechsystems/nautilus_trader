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

import numpy as np
import unittest

from nautilus_trader.core.functions import fast_mean, fast_std
from tests.test_kit.performance import PerformanceHarness


class FunctionPerformanceTests(unittest.TestCase):

    def setUp(self):
        self.values = list(np.random.rand(10))

    def fast_mean(self):
        fast_mean(self.values)

    def fast_std(self):
        fast_std(self.values)

    def np_mean(self):
        np.mean(self.values)

    def np_std(self):
        np.std(self.values)

    def test_fast_mean(self):
        result = PerformanceHarness.profile_function(self.fast_mean, 3, 100000)
        # ~85ms (85064μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 0.1)

    def test_fast_std(self):
        result = PerformanceHarness.profile_function(self.fast_std, 3, 10000)
        # ~19ms (19468μs) minimum of 3 runs @ 10,000 iterations each run.
        self.assertTrue(result < 0.3)

    def test_np_mean(self):
        result = PerformanceHarness.profile_function(self.np_mean, 3, 100000)
        # ~883ms (883964μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 1.0)

    def test_np_std(self):
        result = PerformanceHarness.profile_function(self.np_std, 3, 10000)
        # ~228ms (228805μs) minimum of 3 runs @ 10,000 iterations each run.
        self.assertTrue(result < 1.0)
