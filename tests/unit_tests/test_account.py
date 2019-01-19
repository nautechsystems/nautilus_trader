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

from inv_trader.model.enums import Broker
from inv_trader.model.enums import CurrencyCode
from inv_trader.model.account import Account
from inv_trader.model.events import AccountEvent
from inv_trader.model.identifiers import GUID, AccountId, AccountNumber
from inv_trader.model.money import money_zero, money
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
            money(1000000),
            money(1000000),
            money_zero(),
            money_zero(),
            money_zero(),
            money_zero(),
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
        self.assertEqual(money(1000000), account.free_equity)
        self.assertEqual(money(1000000), account.cash_start_day)
        self.assertEqual(money_zero(), account.cash_activity_day)
        self.assertEqual(money_zero(), account.margin_used_liquidation)
        self.assertEqual(money_zero(), account.margin_used_maintenance)
        self.assertEqual(money_zero(), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)

    def test_can_calculate_free_equity(self):
        # Arrange
        account = Account()

        event = AccountEvent(
            AccountId('FXCM-D102412895'),
            Broker.FXCM,
            AccountNumber('D102412895'),
            CurrencyCode.AUD,
            money(100000),
            money(100000),
            money_zero(),
            money(1000),
            money(2000),
            money_zero(),
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
        self.assertEqual(money(97000), account.free_equity)
        self.assertEqual(money(100000), account.cash_start_day)
        self.assertEqual(money_zero(), account.cash_activity_day)
        self.assertEqual(money(1000), account.margin_used_liquidation)
        self.assertEqual(money(2000), account.margin_used_maintenance)
        self.assertEqual(money_zero(), account.margin_ratio)
        self.assertEqual("", account.margin_call_status)
        self.assertEqual(UNIX_EPOCH, account.last_updated)
