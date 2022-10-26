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

import msgspec
import pytest

from nautilus_trader.adapters.betfair.factories import BetfairLiveDataClientFactory
from nautilus_trader.adapters.betfair.factories import BetfairLiveExecClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.backtest.data.providers import TestInstrumentProvider
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import StrategyId


RAW_CONFIG = msgspec.json.encode(
    {
        "environment": "live",
        "trader_id": "Test-111",
        "log_level": "INFO",
        "exec_engine": {
            "reconciliation_lookback_mins": 1440,
        },
        "data_clients": {
            "BINANCE": {
                "factory_path": "nautilus_trader.adapters.binance.factories:BinanceLiveDataClientFactory",
                "config_path": "nautilus_trader.adapters.binance.config:BinanceDataClientConfig",
                "config": {
                    "account_type": "FUTURES_USDT",
                    "instrument_provider": {"load_all": True},
                },
            }
        },
        "exec_clients": {
            "BINANCE": {
                "factory_path": "nautilus_trader.adapters.binance.factories:BinanceLiveExecClientFactory",
                "config_path": "nautilus_trader.adapters.binance.config:BinanceExecClientConfig",
                "config": {
                    "account_type": "FUTURES_USDT",
                    "instrument_provider": {"load_all": True},
                },
            }
        },
        "timeout_connection": 5.0,
        "timeout_reconciliation": 5.0,
        "timeout_portfolio": 5.0,
        "timeout_disconnection": 5.0,
        "timeout_post_stop": 2.0,
        "strategies": [
            {
                "strategy_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMaker",
                "config_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMakerConfig",
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
        # Arrange, Act
        config = TradingNodeConfig.parse_raw(RAW_CONFIG)
        node = TradingNode(config)

        # Assert
        assert node.trader.id.value == "Test-111"
        assert node.trader.strategy_ids() == [StrategyId("VolatilityMarketMaker-000")]


class TestTradingNodeOperation:
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
            node = TradingNode()
            node.build()
            node.build()

    def test_start_when_not_built_raises_runtime_error(self):
        # Arrange, # Act
        with pytest.raises(RuntimeError):
            node = TradingNode()
            node.start()

    def test_add_data_client_factory(self):
        # Arrange
        node = TradingNode()

        # Act
        node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
        node.build()

        # TODO(cs): Assert existence of client

    def test_add_exec_client_factory(self):
        # Arrange
        node = TradingNode()

        # Act
        node.add_exec_client_factory("BETFAIR", BetfairLiveExecClientFactory)
        node.build()

        # TODO(cs): Assert existence of client

    @pytest.mark.asyncio
    async def test_build_with_multiple_clients(self):
        # Arrange
        node = TradingNode()

        # Act
        node.add_data_client_factory("BETFAIR", BetfairLiveDataClientFactory)
        node.add_exec_client_factory("BETFAIR", BetfairLiveExecClientFactory)
        node.build()

        node.start()
        await asyncio.sleep(1)

        # assert self.node.kernel.data_engine.registered_clients
        # TODO(cs): Assert existence of client

    @pytest.mark.asyncio
    async def test_register_log_sink(self):
        # Arrange
        node = TradingNode()

        sink = []

        # Act
        node.kernel.add_log_sink(sink.append)
        node.build()

        node.start()
        await asyncio.sleep(1)

        # Assert: Log record received
        assert sink[-1]["trader_id"] == node.trader_id.value
        assert sink[-1]["machine_id"] == node.machine_id
        assert sink[-1]["instance_id"] == node.instance_id.value

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange
        node = TradingNode()
        node.build()

        # Act
        node.start()
        await asyncio.sleep(2)

        # Assert
        assert node.trader.is_running

    @pytest.mark.asyncio
    async def test_stop(self):
        # Arrange
        node = TradingNode()
        node.build()
        node.start()
        await asyncio.sleep(2)  # Allow node to start

        # Act
        node.stop()
        await asyncio.sleep(3)  # Allow node to stop

        # Assert
        assert node.trader.is_stopped

    @pytest.mark.skip(reason="setup sandbox environment")
    @pytest.mark.asyncio
    async def test_dispose(self, monkeypatch):
        # Arrange
        monkeypatch.setenv("BINANCE_FUTURES_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_FUTURES_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig.parse_raw(RAW_CONFIG)
        node = TradingNode(config)
        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)

        node.build()
        node.kernel.cache.add_instrument(TestInstrumentProvider.ethusdt_perp_binance())

        node.start()
        await asyncio.sleep(2)  # Allow node to start

        node.stop()
        await asyncio.sleep(2)  # Allow node to stop

        # Act
        node.dispose()
        await asyncio.sleep(1)  # Allow node to dispose

        # Assert
        assert node.trader.is_disposed
