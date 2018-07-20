#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_execution.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import pytz

from datetime import datetime
from decimal import Decimal
from uuid import UUID

from inv_trader.model.enums import Venue, OrderSide, OrderType, OrderStatus, TimeInForce
from inv_trader.model.objects import Symbol, Resolution, QuoteType, BarType, Bar
from inv_trader.model.events import OrderSubmitted, OrderAccepted, OrderRejected, OrderWorking
from inv_trader.model.events import OrderExpired, OrderModified, OrderCancelled, OrderCancelReject
from inv_trader.model.events import OrderPartiallyFilled, OrderFilled
from inv_trader.execution import ExecutionClient, LiveExecClient
from test_kit.stubs import TestStubs
from test_kit.mocks import MockExecClient
from test_kit.objects import ObjectStorer
from test_kit.strategies import TestStrategy1

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue.FXCM)
GBPUSD_FXCM = Symbol('GBPUSD', Venue.FXCM)


class ExecutionClientTests(unittest.TestCase):

    def test_can_register_strategy(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = ExecutionClient()
        exec_client.register_strategy(strategy)

        # Act
        result = strategy._exec_client

        # Assert
        self.assertEqual(exec_client, result)

    def test_can_receive_bars(self):
        # Arrange
        storer = ObjectStorer()
        strategy = TestStrategy1(storer)
        exec_client = MockExecClient()
        exec_client.register_strategy(strategy)

        strategy.start()

        bar_type = BarType(GBPUSD_FXCM,
                           1,
                           Resolution.SECOND,
                           QuoteType.MID)

        bar1 = Bar(
            Decimal('1.00001'),
            Decimal('1.00004'),
            Decimal('1.00003'),
            Decimal('1.00002'),
            100000,
            datetime(1970, 1, 1, 00, 00, 0, 0, pytz.UTC))

        bar2 = Bar(
            Decimal('1.00011'),
            Decimal('1.00014'),
            Decimal('1.00013'),
            Decimal('1.00012'),
            100000,
            datetime(1970, 1, 1, 00, 00, 1, 0, pytz.UTC))

        # Act
        strategy._update_bars(bar_type, bar1)
        strategy._update_bars(bar_type, bar2)
        result = storer.get_store[-1]

        # Assert
        self.assertTrue(isinstance(result, OrderWorking))


class LiveExecClientTests(unittest.TestCase):

    pass
