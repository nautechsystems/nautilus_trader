# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE file.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

import unittest
from datetime import datetime, timezone

from nautilus_trader.model.enums import Currency
from nautilus_trader.model.objects import Price, Volume, Tick
from nautilus_trader.common.guid import TestGuidFactory
from nautilus_trader.common.logging import TestLogger
from nautilus_trader.common.data import DataClient
from nautilus_trader.common.clock import TestClock

from tests.test_kit.stubs import TestStubs

AUDUSD_FXCM = TestStubs.symbol_audusd_fxcm()
USDJPY_FXCM = TestStubs.symbol_usdjpy_fxcm()


class DataClientTests(unittest.TestCase):

    def setUp(self):
        self.client = DataClient(
            tick_capacity=100,
            clock=TestClock(),
            guid_factory=TestGuidFactory(),
            logger=TestLogger())

    def test_get_exchange_rate_returns_correct_rate(self):
        # Arrange
        tick = Tick(USDJPY_FXCM,
                    Price(110.80000, 5),
                    Price(110.80010, 5),
                    Volume(1),
                    Volume(1),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        self.client._handle_tick(tick)

        # Act
        result = self.client.get_exchange_rate(Currency.JPY, Currency.USD)

        # Assert
        self.assertEqual(0.009025266685348969, result)

    def test_can_get_exchange_rate_with_no_conversion(self):
        # Arrange
        tick = Tick(AUDUSD_FXCM,
                    Price(0.80000, 5),
                    Price(0.80010, 5),
                    Volume(1),
                    Volume(1),
                    datetime(2018, 1, 1, 19, 59, 1, 0, timezone.utc))

        self.client._handle_tick(tick)

        # Act
        result = self.client.get_exchange_rate(Currency.AUD, Currency.USD)

        # Assert
        self.assertEqual(0.80005, result)
