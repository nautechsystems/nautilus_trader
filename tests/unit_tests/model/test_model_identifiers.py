# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest

from nautilus_trader.core.types import Identifier
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.identifiers import (
    Symbol,
    Venue,
    Brokerage,
    AccountId,
    TraderId,
    StrategyId,
    OrderId,
    PositionId)


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
        self.assertEqual('some-id', result)

    def test_identifier_repr(self):
        # Arrange
        identifier = Identifier('some-id')

        # Act
        result = repr(identifier)

        # Assert
        self.assertTrue(result.startswith('<Identifier(some-id) object at'))

    def test_mixed_identifier_equality(self):
        # Arrange
        id1 = OrderId('O-123456')
        id2 = PositionId('P-123456')

        # Act
        # Assert
        self.assertTrue(id1 == id1)
        self.assertFalse(id1 == id2)

    def test_symbol_equality(self):
        # Arrange
        symbol1 = Symbol("AUDUSD", Venue('FXCM'))
        symbol2 = Symbol("AUDUSD", Venue('IDEAL_PRO'))
        symbol3 = Symbol("GBPUSD", Venue('FXCM'))

        # Act
        # Assert
        self.assertTrue(symbol1 == symbol1)
        self.assertTrue(symbol1 != symbol2)
        self.assertTrue(symbol1 != symbol3)

    def test_symbol_str_and_repr(self):
        # Arrange
        symbol = Symbol("AUDUSD", Venue('FXCM'))

        # Act
        # Assert
        self.assertEqual("AUDUSD.FXCM", str(symbol))
        self.assertTrue(repr(symbol).startswith("<Symbol(AUDUSD.FXCM) object at"))

    def test_can_parse_symbol_from_string(self):
        # Arrange
        symbol = Symbol('AUDUSD', Venue('FXCM'))

        # Act
        result = Symbol.py_from_string(symbol.value)

        # Assert
        self.assertEqual(symbol, result)

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
        self.assertEqual(trader_id1, TraderId.py_from_string('TESTER-000'))

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
        account_id1 = AccountId('FXCM', '02851908', AccountType.DEMO)
        account_id2 = AccountId('FXCM', '09999999', AccountType.DEMO)

        # Assert
        self.assertEqual(account_id1, account_id1)
        self.assertNotEqual(account_id1, account_id2)
        self.assertEqual('FXCM-02851908-DEMO', account_id1.value)
        self.assertEqual(Brokerage('FXCM'), account_id1.broker)
        self.assertEqual(account_id1, AccountId.py_from_string('FXCM-02851908-DEMO'))
