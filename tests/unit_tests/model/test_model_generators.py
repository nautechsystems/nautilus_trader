# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.model.identifiers import IdTag, OrderId, PositionId
from nautilus_trader.model.generators import OrderIdGenerator, PositionIdGenerator
from nautilus_trader.common.clock import TestClock


class OrderIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.order_id_generator = OrderIdGenerator(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())

    def test_generate_order_id(self):
        # Arrange
        # Act
        result1 = self.order_id_generator.generate()
        result2 = self.order_id_generator.generate()
        result3 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), result1)
        self.assertEqual(OrderId('O-19700101-000000-001-001-2'), result2)
        self.assertEqual(OrderId('O-19700101-000000-001-001-3'), result3)

    def test_can_reset_id_generator(self):
        # Arrange
        self.order_id_generator.generate()
        self.order_id_generator.generate()
        self.order_id_generator.generate()

        # Act
        self.order_id_generator.reset()
        result1 = self.order_id_generator.generate()

        # Assert
        self.assertEqual(OrderId('O-19700101-000000-001-001-1'), result1)


class PositionIdGeneratorTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        self.position_id_generator = PositionIdGenerator(
            id_tag_trader=IdTag('001'),
            id_tag_strategy=IdTag('001'),
            clock=TestClock())

    def test_generate_position_id(self):
        # Arrange
        # Act
        result1 = self.position_id_generator.generate()
        result2 = self.position_id_generator.generate()
        result3 = self.position_id_generator.generate()

        # Assert
        self.assertEqual(PositionId('P-19700101-000000-001-001-1'), result1)
        self.assertEqual(PositionId('P-19700101-000000-001-001-2'), result2)
        self.assertEqual(PositionId('P-19700101-000000-001-001-3'), result3)

    def test_can_reset_id_generator(self):
        # Arrange
        self.position_id_generator.generate()
        self.position_id_generator.generate()
        self.position_id_generator.generate()

        # Act
        self.position_id_generator.reset()
        result1 = self.position_id_generator.generate()

        # Assert
        self.assertEqual(PositionId('P-19700101-000000-001-001-1'), result1)
