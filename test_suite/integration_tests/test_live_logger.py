# -------------------------------------------------------------------------------------------------
# <copyright file="test_live_logger.py" company="Nautech Systems Pty Ltd">
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  The use of this source code is governed by the license as found in the LICENSE.md file.
#  https://nautechsystems.io
# </copyright>
# -------------------------------------------------------------------------------------------------

import redis
import threading
import unittest

from nautilus_trader.model.identifiers import Symbol, Venue, TraderId
from nautilus_trader.common.logger import LogMessage, LogLevel
from nautilus_trader.live.logger import LogStore


from test_kit.stubs import TestStubs

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

        self.trader_id = TraderId('TESTER', '000')
        self.store = LogStore(trader_id=self.trader_id)

        self.test_redis = redis.Redis(host='localhost', port=6379, db=0)

    def tearDown(self):
        # Tests will start failing if redis is not flushed on tear down
        self.test_redis.flushall()  # Comment this line out to preserve data between tests
        pass

    def test_can_store_log_message(self):
        # Arrange
        message = LogMessage(UNIX_EPOCH, LogLevel.WARNING, 'This is a test message.', threading.get_ident())

        # Act
        self.store.store(message)
