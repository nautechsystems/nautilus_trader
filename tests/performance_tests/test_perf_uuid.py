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
import uuid

from nautilus_trader.core.uuid import uuid4
from tests.test_kit.performance import PerformanceHarness


class UUIDPerformanceTests(unittest.TestCase):

    @staticmethod
    def test_make_builtin_uuid():
        PerformanceHarness.profile_function(uuid.uuid4, 100000, 1)
        # ~0.0ms / ~2.1μs / 2067ns minimum of 100,000 runs @ 1 iteration each run.

    @staticmethod
    def test_make_nautilus_uuid():
        PerformanceHarness.profile_function(uuid4, 100000, 1)
        # ~0.0ms / ~0.6μs / 556ns minimum of 100,000 runs @ 1 iteration each run.
