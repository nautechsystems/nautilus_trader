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

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.generators import OrderIdGenerator
from nautilus_trader.model.identifiers import IdTag
from tests.test_kit.performance import PerformanceHarness
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestStubs.symbol_audusd_fxcm()


class OrderPerformanceTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.generator = OrderIdGenerator(IdTag("001"), IdTag("001"), LiveClock())

    def test_order_id_generator(self):
        PerformanceHarness.profile_function(self.generator.generate, 3, 10000)
        # ~30ms (18831μs) minimum of 5 runs @ 10000 iterations
