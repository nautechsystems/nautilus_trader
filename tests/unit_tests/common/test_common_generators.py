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

from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.generators import ClientOrderIdGenerator
from nautilus_trader.common.generators import PositionIdGenerator
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import IdTag
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId


class OrderIdGeneratorTests(unittest.TestCase):
    def setUp(self):
        # Fixture Setup
        self.order_id_generator = ClientOrderIdGenerator(
            id_tag_trader=IdTag("001"), id_tag_strategy=IdTag("001"), clock=TestClock()
        )

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
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=IdTag("001"),
            clock=TestClock(),
        )

    def test_generate_position_id(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(StrategyId("S", "002"))
        result2 = self.position_id_generator.generate(StrategyId("S", "002"))
        result3 = self.position_id_generator.generate(StrategyId("S", "002"))

        # Assert
        self.assertEqual(PositionId("P-19700101-000000-001-002-1"), result1)
        self.assertEqual(PositionId("P-19700101-000000-001-002-2"), result2)
        self.assertEqual(PositionId("P-19700101-000000-001-002-3"), result3)

    def test_generate_position_id_with_flip_appends_correctly(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate(StrategyId("S", "001"))
        result2 = self.position_id_generator.generate(
            StrategyId("S", "002"), flipped=True
        )
        result3 = self.position_id_generator.generate(
            StrategyId("S", "001"), flipped=True
        )

        # Assert
        self.assertEqual(PositionId("P-19700101-000000-001-001-1"), result1)
        self.assertEqual(PositionId("P-19700101-000000-001-002-1F"), result2)
        self.assertEqual(PositionId("P-19700101-000000-001-001-2F"), result3)

    def test_set_count_with_valid_strategy_identifier(self):
        # Arrange
        strategy_id = StrategyId("S", "001")

        # Act
        self.position_id_generator.set_count(strategy_id, 5)

        # Assert
        self.assertEqual(5, self.position_id_generator.get_count(strategy_id))

    def test_get_count_when_strategy_id_has_no_count_returns_zero(self):
        # Arrange
        strategy_id = StrategyId("S", "001")

        # Act
        result = self.position_id_generator.get_count(strategy_id)

        # Assert
        self.assertEqual(0, result)

    def test_reset_id_generator(self):
        # Arrange
        self.position_id_generator.generate(StrategyId("S", "002"))
        self.position_id_generator.generate(StrategyId("S", "002"))
        self.position_id_generator.generate(StrategyId("S", "002"))

        # Act
        self.position_id_generator.reset()
        result1 = self.position_id_generator.generate(StrategyId("S", "002"))

        # Assert
        self.assertEqual(PositionId("P-19700101-000000-001-002-1"), result1)
