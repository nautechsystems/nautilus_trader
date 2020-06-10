# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2020 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  you may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import redis
import threading
import unittest

from nautilus_trader.model.identifiers import Symbol, Venue, TraderId
from nautilus_trader.common.logging import LogMessage, LogLevel
from nautilus_trader.live.logging import LogStore

from tests.test_kit.stubs import UNIX_EPOCH

AUDUSD_FXCM = Symbol('AUDUSD', Venue('FXCM'))
GBPUSD_FXCM = Symbol('GBPUSD', Venue('FXCM'))

UTF8 = 'utf8'
LOCALHOST = "127.0.0.1"

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
