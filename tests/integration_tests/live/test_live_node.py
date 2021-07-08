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

from nautilus_trader.adapters.ccxt.factories import CCXTDataClientFactory
from nautilus_trader.adapters.ccxt.factories import CCXTExecutionClientFactory
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
            },
            "database": {
                "type": "in-memory",
            },
            "strategy": {
                "load_state": True,
                "save_state": True,
            },
            "data_clients": {
                "binance": {
                    "api_key": "BINANCE_API_KEY",  # value is the environment variable name
                    "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
                },
            },
            "exec_clients": {
                "binance": {
                    "api_key": "BINANCE_API_KEY",  # value is the environment variable name
                    "api_secret": "BINANCE_API_SECRET",  # value is the environment variable name
                },
            },
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
            },
            "database": {
                "type": "redis",
                "host": "localhost",
                "port": 6379,
            },
            "strategy": {
                "load_state": True,
                "save_state": True,
            },
            "data_clients": {
                "oanda": {
                    "api_token": "OANDA_API_TOKEN",  # value is the environment variable name
                    "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable name
                },
            },
            "exec_clients": {
                "oanda": {
                    "api_token": "OANDA_API_TOKEN",  # value is the environment variable name
                    "account_id": "OANDA_ACCOUNT_ID",  # value is the environment variable name
                },
            },
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
        self.loop.set_debug(True)
        asyncio.set_event_loop(self.loop)

        config = {
            "trader": {
                "name": "tester",
                "id_tag": "000",
            },
            "logging": {
                "log_level_console": "INF",
            },
            "database": {
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

    def test_get_event_loop_returns_a_loop(self):
        # Arrange
        # Act
        loop = self.node.get_event_loop()

        # Assert
        self.assertTrue(isinstance(loop, asyncio.AbstractEventLoop))

    def test_add_data_client_factory(self):
        self.node.add_data_client_factory("CCXT", CCXTDataClientFactory)
        self.node.build()

    def test_add_exec_client_factory(self):
        self.node.add_exec_client_factory("CCXT", CCXTExecutionClientFactory)
        self.node.build()

    def test_start(self):
        # Arrange
        self.node.build()
        run = threading.Thread(target=self.node.start, daemon=True)
        run.start()

        time.sleep(2)  # Allow node to start

        # Act
        # Assert
        self.assertEqual(ComponentState.RUNNING, self.node.trader.state)
        self.loop.call_soon_threadsafe(self.node.stop)

    def test_stop(self):
        # Arrange
        self.node.build()
        run = threading.Thread(target=self.node.start, daemon=True)
        run.start()

        time.sleep(2)  # Allow node to start
        self.loop.call_soon_threadsafe(self.node.stop)

        time.sleep(3)  # Allow node to stop

        # Act
        # Assert
        self.assertEqual(ComponentState.STOPPED, self.node.trader.state)

    def test_dispose(self):
        # Arrange
        self.node.build()
        run = threading.Thread(target=self.node.start, daemon=True)
        run.start()

        time.sleep(2)  # Allow node to start
        self.loop.call_soon_threadsafe(self.node.stop)

        # Allow node to stop
        time.sleep(3)

        self.node.dispose()

        # Act
        # Assert
        self.assertEqual(ComponentState.DISPOSED, self.node.trader.state)
