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

from collections import deque
import unittest

from tests.test_kit.performance import PerformanceHarness


class PythonDequePerformanceTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.deque = deque(maxlen=1000)
        self.deque.append(1.0)

    def append(self):
        self.deque.append(1.0)

    def peek(self):
        return self.deque[0]

    def test_append(self):
        PerformanceHarness.profile_function(self.append, 100000, 1)
        # ~0.0ms / ~0.2μs / 173ns minimum of 100,000 runs @ 1 iteration each run.

    def test_peek(self):
        PerformanceHarness.profile_function(self.peek, 100000, 1)
        # ~0.0ms / ~0.1μs / 144ns minimum of 100,000 runs @ 1 iteration each run.
