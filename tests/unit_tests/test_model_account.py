# -------------------------------------------------------------------------------------------------
# <copyright file="test_model_account.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from nautilus_trader.core.types import GUID, ValidString
from nautilus_trader.model.enums import Broker
from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.events import AccountEvent
from nautilus_trader.model.identifiers import AccountId, AccountNumber
from nautilus_trader.common.account import Account
from test_kit.stubs import TestStubs

UNIX_EPOCH = TestStubs.unix_epoch()


class AccountTests(unittest.TestCase):

    def test_can_initialize_account_with_event(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            Currency.AUD,
            Money(1000000),
            Money(1000000),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal('0'),
            ValidString('NONE'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(1000000), account.free_equity)
        self.assertEqual(Money(1000000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money.zero(), account.margin_used_liquidation)
        self.assertEqual(Money.zero(), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual('NONE', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_reset_account(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            Currency.AUD,
            Money(1000000),
            Money(1000000),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal('0'),
            ValidString('NONE'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        account.apply(event)

        # Act
        account.reset()

        # Assert
        self.assertFalse(account.initialized)

    def test_can_calculate_free_equity_when_greater_than_zero(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            Currency.AUD,
            Money(100000),
            Money(100000),
            Money.zero(),
            Money(1000),
            Money(2000),
            Decimal('0'),
            ValidString('NONE'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money(97000), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money(1000), account.margin_used_liquidation)
        self.assertEqual(Money(2000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual('NONE', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_zero(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            Currency.AUD,
            Money(20000),
            Money(100000),
            Money.zero(),
            Money.zero(),
            Money(20000),
            Decimal('0'),
            ValidString('NONE'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money.zero(), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money.zero(), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual('NONE', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_negative(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            Currency.AUD,
            Money(20000),
            Money(100000),
            Money.zero(),
            Money(10000),
            Money(20000),
            Decimal('0'),
            ValidString('NONE'),
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(Currency.AUD, account.currency)
        self.assertEqual(Money.zero(), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money(10000), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual('NONE', account.margin_call_status.value)
        self.assertEqual(UNIX_EPOCH, account.last_updated)
