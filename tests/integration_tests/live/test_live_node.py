# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

import asyncio
import threading
import time
import unittest

from nautilus_trader.common.enums import ComponentState
from nautilus_trader.live.node import TradingNode
from nautilus_trader.trading.strategy import TradingStrategy


class TradingNodeConfigurationTests(unittest.TestCase):

    def test_config_with_inmemory_execution_database(self):
        # Arrange
        config = {
            "trader": {
                "name": "tester",
                "id_tag": "000",
            },

            "logging": {
                "log_level_console": "INF",
                "log_level_file": "DBG",
                "log_level_store": "WRN",
            },

            "exec_database": {
                "type": "in-memory",
            },

            "strategy": {
                "load_state": True,
                "save_state": True,
            },

            "data_clients": {},
            "exec_clients": {},
        }

        # Act
        node = TradingNode(
            strategies=[TradingStrategy("000")],
            config=config,
        )

        # Assert
        self.assertIsNotNone(node)

    def test_config_with_redis_execution_database(self):
        # Arrange
        config = {
            "trader": {
                "name": "tester",
                "id_tag": "000",
            },

            "logging": {
                "log_level_console": "INF",
                "log_level_file": "DBG",
                "log_level_store": "WRN",
            },

            "exec_database": {
                "type": "redis",
                "host": "localhost",
                "port": 6379,
            },

            "strategy": {
                "load_state": True,
                "save_state": True,
            },

            "data_clients": {},
            "exec_clients": {},
        }

        # Act
        node = TradingNode(
            strategies=[TradingStrategy("000")],
            config=config,
        )

        # Assert
        self.assertIsNotNone(node)


class TradingNodeOperationTests(unittest.TestCase):

    def setUp(self):
        # Fixture Setup
        # Fresh isolated loop testing pattern
        self.loop = asyncio.new_event_loop()
        asyncio.set_event_loop(self.loop)

        config = {
            "trader": {
                "name": "tester",
                "id_tag": "000",
            },

            "logging": {
                "log_level_console": "INF",
                "log_level_file": "DBG",
                "log_level_store": "WRN",
            },

            "exec_database": {
                "type": "in-memory",
            },

            "strategy": {
                "load_state": True,
                "save_state": True,
            },

            "data_clients": {},
            "exec_clients": {},
        }

        self.node = TradingNode(
            strategies=[TradingStrategy("000")],
            config=config,
        )

    def tearDown(self):
        if self.node.trader.state == ComponentState.RUNNING:
            self.node.stop()

        self.node.shutdown()
        self.node.dispose()

    def test_run(self):
        # Arrange
        run = threading.Thread(target=self.node.start, daemon=True)
        run.start()

        time.sleep(0.5)

        # Act
        # Assert
        self.assertTrue(True)
        self.assertEqual(ComponentState.RUNNING, self.node.trader.state)
