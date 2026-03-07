import asyncio
from types import SimpleNamespace

import msgspec
import pytest

from nautilus_trader.adapters.binance.config import BinanceDataClientConfig
from nautilus_trader.adapters.binance.config import BinanceExecClientConfig
from nautilus_trader.adapters.binance.factories import BinanceLiveDataClientFactory
from nautilus_trader.adapters.binance.factories import BinanceLiveExecClientFactory
from nautilus_trader.config import LoggingConfig
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.live.node import TradingNode
from nautilus_trader.live.node import TradingNodeFatalError
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.test_kit.functions import ensure_all_tasks_completed


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
    def teardown(self):
        ensure_all_tasks_completed()

    def test_config_with_in_memory_execution_database(self, event_loop_for_setup):
        # Arrange
        loop = event_loop_for_setup

        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
        )

        # Act
        node = TradingNode(config=config, loop=loop)

        # Assert
        assert node is not None

    def test_config_with_redis_execution_database(self, event_loop_for_setup):
        # Arrange, Act
        loop = event_loop_for_setup

        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
        )
        node = TradingNode(config=config, loop=loop)

        # Assert
        assert node is not None

    def test_node_config_from_raw(self, event_loop_for_setup):
        # Arrange, Act
        loop = event_loop_for_setup

        config = TradingNodeConfig.parse(RAW_CONFIG)
        node = TradingNode(config=config, loop=loop)

        # Assert
        assert node.trader.id.value == "Test-111"
        assert node.trader.strategy_ids() == [StrategyId("VolatilityMarketMaker-000")]

    def test_setting_instance_id(self, monkeypatch, event_loop_for_setup):
        # Arrange
        loop = event_loop_for_setup

        monkeypatch.setenv("BINANCE_API_KEY", "SOME_API_KEY")
        monkeypatch.setenv("BINANCE_API_SECRET", "SOME_API_SECRET")

        config = TradingNodeConfig.parse(RAW_CONFIG)

        # Act
        node = TradingNode(config=config, loop=loop)
        assert len(node.kernel.instance_id.value) == 36


class TestTradingNodeOperation:
    def teardown(self):
        ensure_all_tasks_completed()

    def test_get_event_loop_returns_a_loop(self, event_loop_for_setup):
        # Arrange
        loop = event_loop_for_setup

        config = TradingNodeConfig(logging=LoggingConfig(bypass_logging=True))
        node = TradingNode(config=config, loop=loop)

        # Act
        loop = node.get_event_loop()

        # Assert
        assert isinstance(loop, asyncio.AbstractEventLoop)
        assert loop == node.kernel.loop

    def test_build_called_twice_raises_runtime_error(self):
        # Arrange
        config = TradingNodeConfig(logging=LoggingConfig(bypass_logging=True))
        node = TradingNode(config=config)
        node.build()

        # Act
        with pytest.raises(RuntimeError):
            node.build()

    @pytest.mark.asyncio
    async def test_run_when_not_built_raises_runtime_error(self):
        # Arrange, Act, Assert
        loop = asyncio.get_running_loop()

        config = TradingNodeConfig(
            logging=LoggingConfig(bypass_logging=True),
            data_clients={
                "BINANCE": BinanceDataClientConfig(),
            },
            exec_clients={
                "BINANCE": BinanceExecClientConfig(),
            },
        )
        node = TradingNode(config=config, loop=loop)

        with pytest.raises(RuntimeError):
            await node.run_async()

    @pytest.mark.asyncio
    async def test_run_and_stop_with_client_factories(self, monkeypatch):
        # Arrange
        loop = asyncio.get_running_loop()
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
        node = TradingNode(config=config, loop=loop)

        node.add_data_client_factory("BINANCE", BinanceLiveDataClientFactory)
        node.add_exec_client_factory("BINANCE", BinanceLiveExecClientFactory)
        node.build()

        # Act, Assert
        node.run()
        await asyncio.sleep(2.0)
        await node.stop_async()

    @pytest.mark.asyncio
    async def test_run_async_raises_for_fatal_startup_without_logging_running(self):
        loop = asyncio.get_running_loop()
        node = TradingNode(
            config=TradingNodeConfig(logging=LoggingConfig(bypass_logging=True)),
            loop=loop,
        )
        node._is_built = True
        entered_task_gather: list[bool] = []

        async def _start_async() -> bool:
            node.kernel._fatal_shutdown_reason = "startup failure: engines failed to connect"
            return False

        async def _done() -> None:
            return None

        monkeypatch = pytest.MonkeyPatch()
        monkeypatch.setattr(node.kernel, "start_async", _start_async)
        def _mark_entered_task_gather():
            entered_task_gather.append(True)
            return loop.create_task(_done())

        monkeypatch.setattr(node.kernel.data_engine, "get_cmd_queue_task", _mark_entered_task_gather)

        with pytest.raises(TradingNodeFatalError, match="startup failure"):
            await node.run_async()

        assert entered_task_gather == []
        monkeypatch.undo()

    @pytest.mark.asyncio
    async def test_run_async_raises_for_fatal_shutdown_after_tasks_complete(self):
        loop = asyncio.get_running_loop()
        node = TradingNode(
            config=TradingNodeConfig(logging=LoggingConfig(bypass_logging=True)),
            loop=loop,
        )
        node._is_built = True
        node.kernel._fatal_shutdown_reason = None
        node.kernel._loop = SimpleNamespace(is_running=lambda: True)

        async def _start_async() -> bool:
            node.kernel._fatal_shutdown_reason = "runtime critical shutdown"
            return True

        async def _done() -> None:
            return None

        monkeypatch = pytest.MonkeyPatch()
        monkeypatch.setattr(node.kernel, "start_async", _start_async)
        monkeypatch.setattr(node.kernel.data_engine, "get_cmd_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.data_engine, "get_req_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.data_engine, "get_res_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.data_engine, "get_data_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.risk_engine, "get_cmd_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.risk_engine, "get_evt_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.exec_engine, "get_cmd_queue_task", lambda: loop.create_task(_done()))
        monkeypatch.setattr(node.kernel.exec_engine, "get_evt_queue_task", lambda: loop.create_task(_done()))

        with pytest.raises(TradingNodeFatalError, match="runtime critical shutdown"):
            await node.run_async()

        monkeypatch.undo()
