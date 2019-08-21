# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_identifiers.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.types import Identifier
from nautilus_trader.common.clock import TestClock
from nautilus_trader.model.identifiers import (
    Brokerage,
    AccountId,
    TraderId,
    StrategyId,
    IdTag,
    OrderId,
    PositionId)
from nautilus_trader.model.generators import OrderIdGenerator, PositionIdGenerator


class IdentifierTests(unittest.TestCase):

    def test_identifier_equality(self):
        # Arrange
        id1 = Identifier('some-id-1')
        id2 = Identifier('some-id-2')

        # Act
        result1 = id1 == id1
        result2 = id1 != id1
        result3 = id1 == id2
        result4 = id1 != id2

        # Assert
        self.assertTrue(result1)
        self.assertFalse(result2)
        self.assertFalse(result3)
        self.assertTrue(result4)

    def test_identifier_to_string(self):
        # Arrange
        identifier = Identifier('some-id')

        # Act
        result = str(identifier)

        # Assert
        self.assertEqual('Identifier(some-id)', result)

    def test_identifier_repr(self):
        # Arrange
        identifier = Identifier('some-id')

        # Act
        result = repr(identifier)

        # Assert
        self.assertTrue(result.startswith('<Identifier(some-id) object at'))

    def test_mixed_identifier_equality(self):
        # Arrange
        identifier_string = 'some-id'
        id1 = OrderId(identifier_string)
        id2 = PositionId(identifier_string)

        # Act
        # Assert
        self.assertTrue(id1 == id1)
        self.assertFalse(id1 == id2)

    def test_trader_identifier(self):
        # Arrange
        # Act
        trader_id1 = TraderId('TESTER', '000')
        trader_id2 = TraderId('TESTER', '001')

        # Assert
        self.assertEqual(trader_id1, trader_id1)
        self.assertNotEqual(trader_id1, trader_id2)
        self.assertEqual('TESTER-000', trader_id1.value)
        self.assertEqual('TESTER', trader_id1.name)
        self.assertEqual(trader_id1, StrategyId.py_from_string('TESTER-000'))

    def test_strategy_identifier(self):
        # Arrange
        # Act
        strategy_id1 = StrategyId('SCALPER', '00')
        strategy_id2 = StrategyId('SCALPER', '01')

        # Assert
        self.assertEqual(strategy_id1, strategy_id1)
        self.assertNotEqual(strategy_id1, strategy_id2)
        self.assertEqual('SCALPER-00', strategy_id1.value)
        self.assertEqual('SCALPER', strategy_id1.name)
        self.assertEqual(strategy_id1, StrategyId.py_from_string('SCALPER-00'))

    def test_account_identifier(self):
        # Arrange
        # Act
        account_id1 = AccountId('FXCM', '02851908')
        account_id2 = AccountId('FXCM', '09999999')

        # Assert
        self.assertEqual(account_id1, account_id1)
        self.assertNotEqual(account_id1, account_id2)
        self.assertEqual('FXCM-02851908', account_id1.value)
        self.assertEqual(Brokerage('FXCM'), account_id1.broker)
        self.assertEqual(account_id1, AccountId.py_from_string('FXCM-02851908'))


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
