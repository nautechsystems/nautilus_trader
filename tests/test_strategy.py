#!/usr/bin/env python3
# -------------------------------------------------------------------------------------------------
# <copyright file="test_strategy.py" company="Invariance Pte">
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
from typing import List


from inv_trader.strategy import TradeStrategy
from inv_trader.objects import Tick, BarType, Bar
from inv_trader.enums import Venue, Resolution, QuoteType


class TradeStrategyTests(unittest.TestCase):

    # Fixture Setup
    def setUp(self):
        # Arrange
        self.strategy = TradeStrategy()

    def test_can_get_strategy_name(self):
        # Act
        # Assert
        self.assertEqual('TradeStrategy', self.strategy.name)



