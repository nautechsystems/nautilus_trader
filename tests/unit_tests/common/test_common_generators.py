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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.generators import OrderIdGenerator
from nautilus_trader.common.generators import PositionIdGenerator
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import PositionId
from tests.test_kit.stubs import TestStubs


class OrderIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_id_generator = OrderIdGenerator(
            id_tag_trader=IdTag("001"),
            id_tag_strategy=IdTag("001"),
            clock=TestClock())

    def test_generate_order_id(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate()
        result2 = self.order_id_generator.generate()
        result3 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(ClientOrderId("O-19700101-000000-001-001-1"), result1)
        self.assertEqual(ClientOrderId("O-19700101-000000-001-001-2"), result2)
        self.assertEqual(ClientOrderId("O-19700101-000000-001-001-3"), result3)

    def test_reset_id_generator(self):
        # Arrange
        self.order_id_generator.generate()
        self.order_id_generator.generate()
        self.order_id_generator.generate()

        # Act
        self.order_id_generator.reset()
        result1 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(ClientOrderId("O-19700101-000000-001-001-1"), result1)


class PositionIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.position_id_generator = PositionIdGenerator(id_tag_trader=IdTag("001"))

    def test_generate_position_id(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())
        result2 = self.position_id_generator.generate(TestStubs.symbol_usdjpy_fxcm())
        result3 = self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())

        # Assert
        self.assertEqual(PositionId("P-001-AUD/USD.FXCM-1"), result1)
        self.assertEqual(PositionId("P-001-USD/JPY.FXCM-1"), result2)
        self.assertEqual(PositionId("P-001-AUD/USD.FXCM-2"), result3)

    def test_generate_position_id_with_flip_appends_correctly(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())
        result2 = self.position_id_generator.generate(TestStubs.symbol_usdjpy_fxcm(), flipped=True)
        result3 = self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm(), flipped=True)

        # Assert
        self.assertEqual(PositionId("P-001-AUD/USD.FXCM-1"), result1)
        self.assertEqual(PositionId("P-001-USD/JPY.FXCM-1F"), result2)
        self.assertEqual(PositionId("P-001-AUD/USD.FXCM-2F"), result3)

    def test_reset_id_generator(self):
        # Arrange
        self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())
        self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())
        self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())

        # Act
        self.position_id_generator.reset()
        result1 = self.position_id_generator.generate(TestStubs.symbol_audusd_fxcm())

        # Assert
        self.assertEqual(PositionId("P-001-AUD/USD.FXCM-1"), result1)
