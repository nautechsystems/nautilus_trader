#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_order.py" company="Invariance Pte">
#  Copyright (C) 2018 Invariance Pte. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  http://www.invariance.com
# </copyright>
# -------------------------------------------------------------------------------------------------

import unittest
import redis
import datetime
import pytz
import time

from decimal import Decimal

from inv_trader.data import LiveDataClient
from inv_trader.objects import Symbol, Tick, BarType, Bar
from inv_trader.enums import Venue, OrderSide, OrderType, OrderStatus
from inv_trader.factories import OrderFactory

AUDUSD_FXCM = Symbol('audusd', Venue.FXCM)
GBPUSD_FXCM = Symbol('gbpusd', Venue.FXCM)


class OrderTests(unittest.TestCase):

    def test_can_initialize_market_order(self):
        # Arrange
        # Act
        order = OrderFactory.market(AUDUSD_FXCM,
                                          'AUDUSD|123456|1',
                                          'SCALPER-01',
                                    OrderSide.BUY,
                                    100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)


    def test_can_initialize_stop_market_order(self):
        # Arrange
        # Act
        order = OrderFactory.stop_market(AUDUSD_FXCM,
                                          'AUDUSD|123456|1',
                                          'SCALPER-01',
                                         OrderSide.BUY,
                                         100000)

        # Assert
        self.assertEqual(OrderType.MARKET, order.type)
        self.assertEqual(OrderStatus.INITIALIZED, order.status)
        self.assertFalse(order.is_complete)

