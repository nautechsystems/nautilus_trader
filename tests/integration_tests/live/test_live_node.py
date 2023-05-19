# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
import unittest.mock

import msgspec
import pytest

# fmt: off
from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersDataClientConfig
from nautilus_trader.adapters.interactive_brokers.config import InteractiveBrokersExecClientConfig
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveDataClientFactory
from nautilus_trader.adapters.interactive_brokers.factories import InteractiveBrokersLiveExecClientFactory
from nautilus_trader.config import CacheDatabaseConfig
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.config.common import InstrumentProviderConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.model.identifiers import StrategyId


# fmt: on


RAW_CONFIG = msgspec.json.encode(
    {
        "environment": "live",
        "trader_id": "Test-111",
        "logging": {"bypass_logging": True},
        "exec_engine": {
            "reconciliation_lookback_mins": 1440,
        },
        "data_clients": {
            "BINANCE": {
                "path": "nautilus_trader.adapters.binance.config:BinanceDataClientConfig",
                "factory": {
                    "path": "nautilus_trader.adapters.binance.factories:BinanceLiveDataClientFactory",
                },
                "config": {
                    "instrument_provider": {
                        "instrument_provider": {"load_all": False},
                    },
                },
            },
        },
        "exec_clients": {
            "BINANCE": {
                "factory": {
                    "path": "nautilus_trader.adapters.binance.factories:BinanceLiveExecClientFactory",
                },
                "path": "nautilus_trader.adapters.binance.config:BinanceExecClientConfig",
                "config": {
                    "instrument_provider": {
                        "instrument_provider": {"load_all": False},
                    },
                },
            },
        },
        "timeout_connection": 5.0,
        "timeout_reconciliation": 5.0,
        "timeout_portfolio": 5.0,
        "timeout_disconnection": 1.0,  # Short timeouts for testing
        "timeout_post_stop": 1.0,  # Short timeouts for testing
        "strategies": [
            {
                "strategy_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMaker",
                "config_path": "nautilus_trader.examples.strategies.volatility_market_maker:VolatilityMarketMakerConfig",
                "config": {
                    "instrument_id": "ETHUSDT-PERP.BINANCE",
                    "bar_type": "ETHUSDT-PERP.BINANCE-1-MINUTE-LAST-EXTERNAL",
                    "atr_period": 20,
                    "atr_multiple": 6.0,
                    "trade_size": "0.01",
                },
            },
        ],
    },
)


class TestTradingNodeConfiguration:
    def test_config_with_in_memory_execution_database(self):
        # Arrange
        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
            cache_database=CacheDatabaseConfig(type="in-memory"),
        )

        # Act
        node = TradingNode(config=config)

        # Assert
        assert node is not None

    def test_config_with_redis_execution_database(self):
        # Arrange, Act
        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
            cache_database=CacheDatabaseConfig(type="in-memory"),
        )
        node = TradingNode(config=config)

        # Assert
        assert node is not None

    def test_node_config_from_raw(self):
        # Arrange, Act
        config = TradingNodeConfig.parse(RAW_CONFIG)
        node = TradingNode(config)

        # Assert
        assert node.trader.id.value == "Test-111"
        assert node.trader.strategy_ids() == [StrategyId("VolatilityMarketMaker-000")]

    @pytest.mark.skip(reason="client configs need to handle params other than those on base class")
    def test_node_build_raw(self, monkeypatch):
        monkeypatch.setenv("BINANCE_FUTURES_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_FUTURES_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig.parse(RAW_CONFIG)
        node = TradingNode(config)
        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
        node.build()

    def test_node_build_objects(self, monkeypatch):
        # Arrange
        config = TradingNodeConfig(
            trader_id="TESTER-001",
            logging=LoggingConfig(bypass_logging=True),
            data_clients={
                "IB": InteractiveBrokersDataClientConfig(),
            },
            exec_clients={
                "IB": InteractiveBrokersExecClientConfig(),
            },
            timeout_connection=90.0,
            timeout_reconciliation=5.0,
            timeout_portfolio=5.0,
            timeout_disconnection=5.0,
            timeout_post_stop=2.0,
        )
        node = TradingNode(config)
        node.add_data_client_factory("IB", InteractiveBrokersLiveDataClientFactory)
        node.add_exec_client_factory("IB", InteractiveBrokersLiveExecClientFactory)

        # Mock factories so nothing actually connects
        from nautilus_trader.adapters.interactive_brokers import factories

        mock_data_factory = (
            factories.InteractiveBrokersLiveDataClientFactory.create
        ) = unittest.mock.MagicMock()
        mock_exec_factory = (
            factories.InteractiveBrokersLiveExecClientFactory.create
        ) = unittest.mock.MagicMock()

        # Act - lazy way of mocking the whole client
        with pytest.raises(TypeError):
            node._builder.build_data_clients(node._config.data_clients)
        with pytest.raises(TypeError):
            node._builder.build_exec_clients(node._config.exec_clients)

        # Assert
        assert mock_data_factory.called
        assert mock_exec_factory.called

    def test_setting_instance_id(self, monkeypatch):
        # Arrange
        monkeypatch.setenv("BINANCE_FUTURES_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_FUTURES_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig.parse(RAW_CONFIG)

        # Act
        node = TradingNode(config)
        assert len(node.kernel.instance_id.value) == 36


class TestTradingNodeOperation:
    def test_get_event_loop_returns_a_loop(self):
        # Arrange
        config = TradingNodeConfig(logging=LoggingConfig(bypass_logging=True))
        node = TradingNode(config=config)

        # Act
        loop = node.get_event_loop()

        # Assert
        assert isinstance(loop, asyncio.AbstractEventLoop)

    def test_build_called_twice_raises_runtime_error(self):
        # Arrange, # Act
        with pytest.raises(RuntimeError):
            config = TradingNodeConfig(logging=LoggingConfig(bypass_logging=True))
            node = TradingNode(config=config)
            node.build()
            node.build()

    @pytest.mark.asyncio()
    async def test_run_when_not_built_raises_runtime_error(self):
        # Arrange, Act, Assert
        with pytest.raises(RuntimeError):
            config = TradingNodeConfig(
                logging=LoggingConfig(bypass_logging=True),
                data_clients={
                    "BINANCE": BinanceDataClientConfig(),
                },
                exec_clients={
                    "BINANCE": BinanceExecClientConfig(),
                },
            )
            node = TradingNode(config=config)
            await node.run_async()

    @pytest.mark.asyncio()
    async def test_run_and_stop_with_client_factories(self, monkeypatch):
        # Arrange
        monkeypatch.setenv("BINANCE_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
            data_clients={
                "BINANCE": BinanceDataClientConfig(),
            },
            exec_clients={
                "BINANCE": BinanceExecClientConfig(),
            },
            timeout_disconnection=1.0,  # Short timeout for testing
            timeout_post_stop=1.0,  # Short timeout for testing
        )
        node = TradingNode(config=config)

        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
        node.build()

        # Act, Assert
        node.run()
        await asyncio.sleep(2.0)
        await node.stop_async()

    @pytest.mark.skip(reason="WIP: continue adding tests here")
    @pytest.mark.asyncio()
    async def test_run_stop_and_dispose(self, monkeypatch):
        # Arrange
        monkeypatch.setenv("BINANCE_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
            data_clients={
                "BINANCE": BinanceDataClientConfig(
                    instrument_provider=InstrumentProviderConfig(load_all=False),
                ),
            },
            exec_clients={
                "BINANCE": BinanceExecClientConfig(
                    instrument_provider=InstrumentProviderConfig(load_all=False),
                ),
            },
            timeout_disconnection=1.0,  # Short timeout for testing
            timeout_post_stop=1.0,  # Short timeout for testing
        )
        node = TradingNode(config=config)

        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
        node.build()

        # Act
        node.run()
        await asyncio.sleep(2.0)
        await node.stop_async()

        # await asyncio.sleep(2.0)
        # node.dispose()
        # assert node.trader.is_disposed
