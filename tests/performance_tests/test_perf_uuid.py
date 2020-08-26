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
import uuid

from nautilus_trader.core.uuid import uuid4
from tests.test_kit.performance import PerformanceHarness


class UUIDTests:

    @staticmethod
    def make_builtin_uuid():
        uuid.uuid4()

    @staticmethod
    def make_nautilus_uuid():
        uuid4()


class UUIDPerformanceTests(unittest.TestCase):

    def test_make_builtin_uuid(self):
        result = PerformanceHarness.profile_function(UUIDTests.make_builtin_uuid, 3, 100000)
        # ~279ms (279583μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 1.2)

    def test_make_nautilus_uuid(self):
        result = PerformanceHarness.profile_function(UUIDTests.make_nautilus_uuid, 3, 100000)
        # ~235ms (235752μs) minimum of 3 runs @ 100,000 iterations each run.
        self.assertTrue(result < 1.2)
