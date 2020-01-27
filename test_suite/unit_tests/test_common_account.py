# -------------------------------------------------------------------------------------------------
# <copyright file="test_common_account.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.core.decimal import Decimal
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.events import AccountStateEvent
from nautilus_trader.model.identifiers import Brokerage, AccountNumber, AccountId
from nautilus_trader.common.account import Account
from test_kit.stubs import UNIX_EPOCH


class AccountTests(unittest.TestCase):

    def test_can_initialize_account_with_event(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string('FXCM-123456-SIMULATED'),
            Currency.AUD,
            Money(1000000),
            Money(1000000),
            Money(0),
            Money(0),
            Money(0),
            Decimal(0),
            ValidString('N'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string('FXCM-123456-SIMULATED'), account.id)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(1000000), account.free_equity)
        self.assertEqual(Money(1000000), account.cash_start_day)
        self.assertEqual(Money(0), account.cash_activity_day)
        self.assertEqual(Money(0), account.margin_used_liquidation)
        self.assertEqual(Money(0), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual('N', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_greater_than_zero(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string('FXCM-123456-SIMULATED'),
            Currency.AUD,
            Money(100000),
            Money(100000),
            Money(0),
            Money(1000),
            Money(2000),
            Decimal(0),
            ValidString('N'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string('FXCM-123456-SIMULATED'), account.id)
        self.assertEqual(Brokerage('FXCM'), account.broker)
        self.assertEqual(AccountNumber('123456'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(97000), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money(0), account.cash_activity_day)
        self.assertEqual(Money(1000), account.margin_used_liquidation)
        self.assertEqual(Money(2000), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual('N', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_zero(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string('FXCM-123456-SIMULATED'),
            Currency.AUD,
            Money(20000),
            Money(100000),
            Money(0),
            Money(0),
            Money(20000),
            Decimal(0),
            ValidString('N'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string('FXCM-123456-SIMULATED'), account.id)
        self.assertEqual(Brokerage('FXCM'), account.broker)
        self.assertEqual(AccountNumber('123456'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(0), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money(0), account.cash_activity_day)
        self.assertEqual(Money(0), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual('N', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_negative(self):
        # Arrange
        event = AccountStateEvent(
            AccountId.py_from_string('FXCM-123456-SIMULATED'),
            Currency.AUD,
            Money(20000),
            Money(100000),
            Money(0),
            Money(10000),
            Money(20000),
            Decimal(0),
            ValidString('N'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account = Account(event)

        # Assert
        self.assertEqual(AccountId.py_from_string('FXCM-123456-SIMULATED'), account.id)
        self.assertEqual(Brokerage('FXCM'), account.broker)
        self.assertEqual(AccountNumber('123456'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(0), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money(0), account.cash_activity_day)
        self.assertEqual(Money(10000), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal(0), account.margin_ratio)
        self.assertEqual('N', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)
