# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_logger.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2019 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import threading
import unittest

from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.live.logger import LogStore
from test_kit.stubs import TestStubs
from nautilus_trader.common.logger import LogMessage, LogLevel
from nautilus_trader.model.objects import Quantity, Venue, Symbol, Price, Money

UNIX_EPOCH = TestStubs.unix_epoch()
AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))

UTF8 = 'utf8'
LOCAL_HOST = "127.0.0.1"

# Requirements:
#    - A Redis instance listening on the default port 6379


class LogStoreTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup

        self.trader_id = TraderId('000')
        self.store = LogStore(trader_id=self.trader_id)

    def test_can_store_order_event(self):
        # Arrange
        message = LogMessage(UNIX_EPOCH, LogLevel.WARNING, 'This is a test message.', threading.get_ident())

        # Act
        self.store.store(message)
