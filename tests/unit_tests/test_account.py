#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_account.py" company="Invariance Pte">
#  Copyright (C) 2018-2019 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import uuid

from decimal import Decimal

from inv_trader.model.enums import Broker
from inv_trader.model.enums import CurrencyCode
from inv_trader.model.objects import Money
from inv_trader.model.events import AccountEvent
from inv_trader.model.identifiers import GUID, AccountId, AccountNumber
from inv_trader.common.account import Account
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
            CurrencyCode.AUD,
            Money(1000000),
            Money(1000000),
            Money.zero(),
            Money.zero(),
            Money.zero(),
            Decimal('0'),
            "",
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(CurrencyCode.AUD, account.currency)
        self.assertEqual(Money(1000000), account.free_equity)
        self.assertEqual(Money(1000000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money.zero(), account.margin_used_liquidation)
        self.assertEqual(Money.zero(), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_greater_than_zero(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            CurrencyCode.AUD,
            Money(100000),
            Money(100000),
            Money.zero(),
            Money(1000),
            Money(2000),
            Decimal('0'),
            "",
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(CurrencyCode.AUD, account.currency)
        self.assertEqual(Money(97000), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money(1000), account.margin_used_liquidation)
        self.assertEqual(Money(2000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_zero(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            CurrencyCode.AUD,
            Money(20000),
            Money(100000),
            Money.zero(),
            Money(0),
            Money(20000),
            Decimal('0'),
            "",
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(CurrencyCode.AUD, account.currency)
        self.assertEqual(Money.zero(), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money(0), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity_when_negative(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            CurrencyCode.AUD,
            Money(20000),
            Money(100000),
            Money.zero(),
            Money(10000),
            Money(20000),
            Decimal('0'),
            "",
            GUID(uuid.uuid4()),
            UNIX_EPOCH)

        # Act
        account.apply(event)

        # Assert
        self.assertTrue(account.initialized)
        self.assertEqual(AccountId('FXCM-D102412895'), account.id)
        self.assertEqual(Broker.FXCM, account.broker)
        self.assertEqual(AccountNumber('D102412895'), account.account_number)
        self.assertEqual(CurrencyCode.AUD, account.currency)
        self.assertEqual(Money.zero(), account.free_equity)
        self.assertEqual(Money(100000), account.cash_start_day)
        self.assertEqual(Money.zero(), account.cash_activity_day)
        self.assertEqual(Money(10000), account.margin_used_liquidation)
        self.assertEqual(Money(20000), account.margin_used_maintenance)
        self.assertEqual(Decimal('0'), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)
