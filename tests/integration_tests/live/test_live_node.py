# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import orjson
import pytest

from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.infrastructure.config import CacheDatabaseConfig
from nautilus_trader.live.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode


class TestTradingNodeConfiguration:
    def test_config_with_inmemory_execution_database(self):
        # Arrange
        config = TradingNodeConfig(cache_database=CacheDatabaseConfig(type="in-memory"))

        # Act
        node = TradingNode(config=config)

        # Assert
        assert node is not None

    def test_config_with_redis_execution_database(self):
        # Arrange, Act
        node = TradingNode()

        # Assert
        assert node is not None

    def test_node_config_from_raw(self):
        # Arrange
        raw = orjson.dumps(
            {
                "trader_id": "Test-111",
                "log_level": "INFO",
                "exec_engine": {
                    "reconciliation_lookback_mins": 1440,
                },
                "data_clients": {
                    "BINANCE": {
                        "class": "BinanceDataClientConfig",
                        "account_type": "FUTURES_USDT",
                        "instrument_provider": {"load_all": True},
                    },
                },
                "exec_clients": {
                    "BINANCE": {
                        "class": "BinanceExecClientConfig",
                        "account_type": "FUTURES_USDT",
                        "instrument_provider": {"load_all": True},
                    }
                },
                "timeout_connection": 5.0,
                "timeout_reconciliation": 5.0,
                "timeout_portfolio": 5.0,
                "timeout_disconnection": 5.0,
                "check_residuals_delay": 2.0,
                "strategies": [
                    {
                        "path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMaker",
                        "config": {
                            "instrument_id": "ETHUSDT-PERP.BINANCE",
                            "bar_type": "ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                            "atr_period": "20",
                            "atr_multiple": "6.0",
                            "trade_size": "0.01",
                        },
                    }
                ],
            }
        )
        # Act
        config = TradingNodeConfig.parse_raw(raw)
        node = TradingNode(config)

        # Assert
        assert node.trader.id == "Test-111"


class TestTradingNodeOperation:
    def setup(self):
        # Fixture Setup
        self.node = TradingNode()

    def test_get_event_loop_returns_a_loop(self):
        # Arrange
        node = TradingNode()

        # Act
        loop = node.get_event_loop()

        # Assert
        assert isinstance(loop, asyncio.AbstractEventLoop)

    def test_build_called_twice_raises_runtime_error(self):
        # Arrange, # Act
        with pytest.raises(RuntimeError):
            self.node.build()
            self.node.build()

    def test_start_when_not_built_raises_runtime_error(self):
        # Arrange, # Act
        with pytest.raises(RuntimeError):
            self.node.start()

    def test_add_data_client_factory(self):
        # Arrange, # Act
        self.node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
        self.node.build()

        # TODO(cs): Assert existence of client

    def test_add_exec_client_factory(self):
        # Arrange, # Act
        self.node.add_exec_client_factory("BETFAIR", BetfairLiveExecClientFactory)
        self.node.build()

        # TODO(cs): Assert existence of client

    def test_build_with_multiple_clients(self):
        # Arrange, # Act
        self.node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
        self.node.add_exec_client_factory("BETFAIR", BetfairLiveExecClientFactory)
        self.node.build()

        # TODO(cs): Assert existence of client

    @pytest.mark.asyncio
    async def test_register_log_sink(self):
        # Arrange
        sink = []

        # Act
        self.node.add_log_sink(sink.append)
        self.node.build()

        self.node.start()
        await asyncio.sleep(1)

        # Assert: Log record received
        assert sink[-1]["trader_id"] == self.node.trader_id.value
        assert sink[-1]["machine_id"] == self.node.machine_id
        assert sink[-1]["instance_id"] == self.node.instance_id.value

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange
        self.node.build()

        # Act
        self.node.start()
        await asyncio.sleep(2)

        # Assert
        assert self.node.trader.is_running

    @pytest.mark.asyncio
    async def test_stop(self):
        # Arrange
        self.node.build()
        self.node.start()
        await asyncio.sleep(2)  # Allow node to start

        # Act
        self.node.stop()
        await asyncio.sleep(3)  # Allow node to stop

        # Assert
        assert self.node.trader.is_stopped

    @pytest.mark.skip(reason="refactor TradingNode coroutines")
    @pytest.mark.asyncio
    async def test_dispose(self):
        # Arrange
        self.node.build()
        self.node.start()
        await asyncio.sleep(2)  # Allow node to start

        self.node.stop()
        await asyncio.sleep(2)  # Allow node to stop

        # Act
        self.node.dispose()
        await asyncio.sleep(1)  # Allow node to dispose

        # Assert
        assert self.node.trader.is_disposed
