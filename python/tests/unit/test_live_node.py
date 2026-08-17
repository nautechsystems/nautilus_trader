# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.common import Cache
from nautilus_trader.common import CacheConfig
from nautilus_trader.common import ComponentState
from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.common import MessageBusConfig
from nautilus_trader.live import LiveDataEngineConfig
from nautilus_trader.live import LiveExecEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveNodeConfig
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.model import ActorId
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.portfolio import Portfolio
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import ExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableControllerConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from tests.unit.common.actor import ControllerRegistrationProbe
from tests.unit.common.actor import LifecycleProbeStrategy


@pytest.fixture(scope="module")
def live_node():
    trader_id = TraderId("TESTER-001")
    return LiveNode.builder("TEST", trader_id, Environment.SANDBOX).build()


class RequiredConfigLiveActorConfig(DataActorConfig):
    def __init__(
        self,
        required_label: str,
        actor_id=None,
        log_events: bool = True,
        log_commands: bool = True,
    ):
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.required_label = required_label


class RequiredConfigLiveActor(DataActor):
    received_actor_id: str | None = None
    received_label: str | None = None

    def __init__(self, config: RequiredConfigLiveActorConfig):
        super().__init__()
        type(self).received_actor_id = str(config.actor_id)
        type(self).received_label = config.required_label


class LifecycleExecutionAlgorithm(ExecutionAlgorithm):
    start_observations = []

    def on_start(self):
        type(self).start_observations.append((self.state, self.is_running()))


class FirstDefaultLiveExecutionAlgorithm(ExecutionAlgorithm):
    pass


class SecondDefaultLiveExecutionAlgorithm(ExecutionAlgorithm):
    pass


class FirstDefaultLiveStrategy(Strategy):
    pass


class SecondDefaultLiveStrategy(Strategy):
    pass


class DefaultIdLiveStrategy(Strategy):
    instances = []

    def __init__(self, config: StrategyConfig | None = None):
        super().__init__(config)
        type(self).instances.append(self)


class DefaultIdLiveActor(DataActor):
    instances = []

    def __init__(self, config: DataActorConfig | None = None):
        super().__init__(config)
        type(self).instances.append(self)


class SecondDefaultIdLiveActor(DefaultIdLiveActor):
    pass


class NonForwardingLiveActor(DataActor):
    instances = []

    def __init__(self, config=None):
        # Deliberately does not forward to `super().__init__()`
        type(self).instances.append(self)


class SecondNonForwardingLiveActor(NonForwardingLiveActor):
    pass


def test_importable_actor_config_construction():
    config = ImportableActorConfig(
        actor_path="tests.unit.common.actor:TestActor",
        config_path="tests.unit.common.actor:TestActorConfig",
        config={"actor_id": "TEST-001"},
    )

    assert config.actor_path == "tests.unit.common.actor:TestActor"
    assert config.config_path == "tests.unit.common.actor:TestActorConfig"
    assert config.config == {"actor_id": "TEST-001"}


def test_importable_actor_config_empty():
    config = ImportableActorConfig(
        actor_path="module:Class",
        config_path="module:Config",
        config={},
    )

    assert config.actor_path == "module:Class"
    assert config.config == {}


def test_importable_strategy_config_construction():
    config = ImportableStrategyConfig(
        strategy_path="tests.unit.common.actor:TestStrategy",
        config_path="nautilus_trader.trading:StrategyConfig",
        config={"strategy_id": "S-001"},
    )

    assert config.strategy_path == "tests.unit.common.actor:TestStrategy"
    assert config.config_path == "nautilus_trader.trading:StrategyConfig"
    assert config.config == {"strategy_id": "S-001"}


def test_importable_controller_config_construction():
    config = ImportableControllerConfig(
        controller_path="tests.unit.common.actor:StrategyCreatingController",
        config_path="tests.unit.common.actor:TestControllerConfig",
        config={"actor_id": "Controller-001"},
    )

    assert config.controller_path == "tests.unit.common.actor:StrategyCreatingController"
    assert config.config_path == "tests.unit.common.actor:TestControllerConfig"
    assert config.config == {"actor_id": "Controller-001"}


def test_live_node_config_registers_importable_controller():
    ControllerRegistrationProbe.reset()
    trader_id = TraderId("TESTER-003")
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=trader_id,
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            controller=ImportableControllerConfig(
                controller_path="tests.unit.common.actor:ControllerRegistrationProbe",
                config_path="tests.unit.common.actor:ControllerRegistrationProbeConfig",
                config={"actor_id": "Controller-001"},
            ),
        ),
    )

    assert node.trader_id == trader_id
    assert ControllerRegistrationProbe.constructed == 1
    assert ControllerRegistrationProbe.received_actor_id == "Controller-001"


def test_live_node_exposes_cache_and_portfolio_inspection():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-010"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
        ),
    )

    try:
        assert isinstance(node.cache, Cache)
        assert node.cache.instrument_ids() == []
        assert node.cache.orders() == []
        assert node.cache.positions() == []
        assert isinstance(node.portfolio, Portfolio)
        assert node.portfolio.is_initialized() is False
    finally:
        node.dispose()


@pytest.mark.asyncio
async def test_live_node_start_stop_dispose_local():
    LifecycleProbeStrategy.reset()
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-004"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            msgbus=MessageBusConfig(external_streams=["signals"]),
            timeout_connection_secs=0,
            timeout_reconciliation_secs=0,
            timeout_portfolio_secs=0,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
            timeout_shutdown_secs=0,
        ),
    )
    node.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="tests.unit.common.actor:LifecycleProbeStrategy",
            config_path="nautilus_trader.trading:StrategyConfig",
            config={},
        ),
    )

    handle = node.handle()

    try:
        assert handle.is_running is False

        task = asyncio.create_task(node.run_async())
        await asyncio.sleep(0.1)
        assert handle.is_running is True

        handle.stop()
        async with asyncio.timeout(10.0):
            await task

        assert handle.is_running is False
    finally:
        node.dispose()

    assert node.is_running is False
    assert LifecycleProbeStrategy.started == 1
    assert LifecycleProbeStrategy.stopped == 1
    assert LifecycleProbeStrategy.disposed == 1


def test_live_node_dispose_before_start_twice_does_not_raise():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-006"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
        ),
    )

    node.dispose()
    node.dispose()


def test_live_node_stop_before_start_raises():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-008"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
        ),
    )

    try:
        with pytest.raises(RuntimeError, match="LiveNode is not running"):
            node.stop()
    finally:
        node.dispose()


@pytest.mark.asyncio
async def test_live_node_run_after_dispose_raises():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-009"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            timeout_connection_secs=0,
            timeout_reconciliation_secs=0,
            timeout_portfolio_secs=0,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
            timeout_shutdown_secs=0,
        ),
    )
    node.dispose()

    try:
        with pytest.raises(RuntimeError, match="cannot be run from state Stopped"):
            node.run_async()
    finally:
        node.dispose()


@pytest.mark.asyncio
async def test_live_node_strategy_start_failure_disposes_resources():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-007"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            timeout_connection_secs=1,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
        ),
    )
    node.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="tests.unit.common.actor:FailingStartStrategy",
            config_path="nautilus_trader.trading:StrategyConfig",
            config={},
        ),
    )

    handle = node.handle()

    with pytest.raises(RuntimeError, match="simulated live node strategy start failure"):
        await node.run_async()

    assert handle.is_running is False
    node.dispose()


def test_importable_exec_algorithm_config_construction():
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="tests.unit.common.actor:TestExecAlgorithm",
        config_path="tests.unit.common.actor:TestExecAlgorithmConfig",
        config={"actor_id": "ALGO-001"},
    )

    assert config.exec_algorithm_path == "tests.unit.common.actor:TestExecAlgorithm"
    assert config.config_path == "tests.unit.common.actor:TestExecAlgorithmConfig"
    assert config.config == {"actor_id": "ALGO-001"}


def test_importable_exec_algorithm_config_empty():
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="module:Class",
        config_path="module:Config",
        config={},
    )

    assert config.exec_algorithm_path == "module:Class"
    assert config.config == {}


def test_builder_accepts_supported_runtime_configs():
    trader_id = TraderId("TESTER-002")
    cache_config = CacheConfig(
        None,
        False,
        None,
        None,
        True,
        False,
        False,
        True,
        10000,
        10000,
        True,
        True,
    )

    node = (
        LiveNode.builder("TEST", trader_id, Environment.SANDBOX)
        .with_cache_config(cache_config)
        .with_portfolio_config(PortfolioConfig())
        .with_data_engine_config(LiveDataEngineConfig(time_bars_build_with_no_updates=False))
        .with_risk_engine_config(LiveRiskEngineConfig(bypass=True))
        .with_exec_engine_config(LiveExecEngineConfig(reconciliation=False))
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_builder_rejects_unregistered_cache_database_factory():
    builder = LiveNode.builder("TEST", TraderId("TESTER-003"), Environment.SANDBOX)

    with pytest.raises(
        NotImplementedError,
        match="No cache database factory extractor registered for 'dict'",
    ):
        builder.with_cache_database_factory({})


def test_add_actor_from_config_registers(live_node):
    config = ImportableActorConfig(
        actor_path="tests.unit.common.actor:TestActor",
        config_path="tests.unit.common.actor:TestActorConfig",
        config={},
    )

    live_node.add_actor_from_config(config)


def test_add_actor_from_config_accepts_required_subclass_kwargs(live_node):
    RequiredConfigLiveActor.received_actor_id = None
    RequiredConfigLiveActor.received_label = None
    config = ImportableActorConfig(
        actor_path="tests.unit.test_live_node:RequiredConfigLiveActor",
        config_path="tests.unit.test_live_node:RequiredConfigLiveActorConfig",
        config={
            "actor_id": "LIVE-CONFIG-ACTOR-001",
            "required_label": "configured",
        },
    )

    live_node.add_actor_from_config(config)

    assert RequiredConfigLiveActor.received_actor_id == "LIVE-CONFIG-ACTOR-001"
    assert RequiredConfigLiveActor.received_label == "configured"


def test_add_actor_from_config_rejects_invalid_path(live_node):
    config = ImportableActorConfig(
        actor_path="no_colon_here",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="actor_path must be in format"):
        live_node.add_actor_from_config(config)


def test_add_actor_from_config_rejects_nonexistent_module(live_node):
    config = ImportableActorConfig(
        actor_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_actor_from_config(config)


def test_add_strategy_from_config_registers(live_node):
    config = ImportableStrategyConfig(
        strategy_path="tests.unit.common.actor:TestStrategy",
        config_path="nautilus_trader.trading:StrategyConfig",
        config={},
    )

    live_node.add_strategy_from_config(config)


def test_add_strategy_from_config_rejects_invalid_path(live_node):
    config = ImportableStrategyConfig(
        strategy_path="no_colon_here",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="strategy_path must be in format"):
        live_node.add_strategy_from_config(config)


def test_add_strategy_from_config_rejects_nonexistent_module(live_node):
    config = ImportableStrategyConfig(
        strategy_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_strategy_from_config(config)


def test_add_exec_algorithm_from_config_registers(live_node):
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="tests.unit.common.actor:TestExecAlgorithm",
        config_path="tests.unit.common.actor:TestExecAlgorithmConfig",
        config={},
    )

    live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_from_config_registers_v2_instance(live_node):
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="tests.unit.test_live_node:LifecycleExecutionAlgorithm",
        config_path="nautilus_trader.trading:ExecutionAlgorithmConfig",
        config={"exec_algorithm_id": "PY-LIVE-CONFIG"},
    )

    live_node.add_exec_algorithm_from_config(config)

    with pytest.raises(RuntimeError, match="'PY-LIVE-CONFIG' is already registered"):
        live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_registers_constructed_instance(live_node):
    exec_algorithm_id = ExecAlgorithmId("PY-LIVE-CONSTRUCTED")
    LifecycleExecutionAlgorithm.start_observations = []
    exec_algorithm = LifecycleExecutionAlgorithm(
        ExecutionAlgorithmConfig(
            exec_algorithm_id=exec_algorithm_id,
            log_events=False,
            log_commands=False,
        ),
    )

    live_node.add_exec_algorithm(exec_algorithm)

    assert exec_algorithm.exec_algorithm_id == exec_algorithm_id
    assert exec_algorithm.is_registered() is True
    assert exec_algorithm.is_ready() is True
    assert exec_algorithm.portfolio.is_initialized() is False

    exec_algorithm.start()
    assert exec_algorithm.is_running() is True
    assert LifecycleExecutionAlgorithm.start_observations == [(ComponentState.STARTING, False)]

    exec_algorithm.stop()
    assert exec_algorithm.is_stopped() is True

    exec_algorithm.resume()
    assert exec_algorithm.is_running() is True

    exec_algorithm.degrade()
    assert exec_algorithm.is_degraded() is True

    exec_algorithm.resume()
    assert exec_algorithm.is_running() is True

    exec_algorithm.stop()
    assert exec_algorithm.is_stopped() is True

    exec_algorithm.reset()
    assert exec_algorithm.is_ready() is True

    duplicate = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=exec_algorithm_id),
    )

    with pytest.raises(RuntimeError, match="'PY-LIVE-CONSTRUCTED' is already registered"):
        live_node.add_exec_algorithm(duplicate)


def test_add_exec_algorithms_registers_distinct_class_derived_ids(live_node):
    first = FirstDefaultLiveExecutionAlgorithm()
    second = SecondDefaultLiveExecutionAlgorithm()

    live_node.add_exec_algorithm(first)
    live_node.add_exec_algorithm(second)

    assert first.exec_algorithm_id == ExecAlgorithmId("FirstDefaultLiveExecutionAlgorithm")
    assert first.is_registered() is True
    assert second.exec_algorithm_id == ExecAlgorithmId("SecondDefaultLiveExecutionAlgorithm")
    assert second.is_registered() is True


def test_add_strategies_registers_distinct_class_derived_ids():
    node = LiveNode.builder("TEST", TraderId("TESTER-020"), Environment.SANDBOX).build()
    first = FirstDefaultLiveStrategy()
    second = SecondDefaultLiveStrategy()

    try:
        node.add_strategy(first)
        node.add_strategy(second)

        assert first.strategy_id == StrategyId("FirstDefaultLiveStrategy-000")
        assert first.state() == ComponentState.READY
        assert second.strategy_id == StrategyId("SecondDefaultLiveStrategy-001")
        assert second.state() == ComponentState.READY
    finally:
        node.dispose()


def test_add_strategy_from_config_derives_class_derived_id():
    node = LiveNode.builder("TEST", TraderId("TESTER-021"), Environment.SANDBOX).build()
    DefaultIdLiveStrategy.instances.clear()
    config = ImportableStrategyConfig(
        strategy_path="tests.unit.test_live_node:DefaultIdLiveStrategy",
        config_path="nautilus_trader.trading:StrategyConfig",
        config={},
    )

    try:
        node.add_strategy_from_config(config)

        registered = DefaultIdLiveStrategy.instances[-1]
        assert registered.strategy_id == StrategyId("DefaultIdLiveStrategy-000")
        assert registered.state() == ComponentState.READY
    finally:
        node.dispose()


def test_add_actors_from_config_derive_distinct_class_derived_ids():
    node = LiveNode.builder("TEST", TraderId("TESTER-022"), Environment.SANDBOX).build()
    DefaultIdLiveActor.instances.clear()
    configs = [
        ImportableActorConfig(
            actor_path=f"tests.unit.test_live_node:{class_name}",
            config_path="nautilus_trader.common:DataActorConfig",
            config={},
        )
        for class_name in ("DefaultIdLiveActor", "SecondDefaultIdLiveActor")
    ]

    try:
        for config in configs:
            node.add_actor_from_config(config)

        first, second = DefaultIdLiveActor.instances
        assert first.actor_id == ActorId("DefaultIdLiveActor")
        assert first.state() == ComponentState.READY
        assert second.actor_id == ActorId("SecondDefaultIdLiveActor")
        assert second.state() == ComponentState.READY
    finally:
        node.dispose()


def test_add_actors_from_config_derive_ids_without_a_forwarding_constructor():
    # Without class-derived IDs at registration both fall back to the shared `DataActor`
    # default and the second registration is rejected as a duplicate
    node = LiveNode.builder("TEST", TraderId("TESTER-023"), Environment.SANDBOX).build()
    configs = [
        ImportableActorConfig(
            actor_path=f"tests.unit.test_live_node:{class_name}",
            config_path="nautilus_trader.common:DataActorConfig",
            config={},
        )
        for class_name in ("NonForwardingLiveActor", "SecondNonForwardingLiveActor")
    ]

    NonForwardingLiveActor.instances.clear()

    try:
        for config in configs:
            node.add_actor_from_config(config)

        first, second = NonForwardingLiveActor.instances
        assert first.actor_id == ActorId("NonForwardingLiveActor")
        assert second.actor_id == ActorId("SecondNonForwardingLiveActor")
        assert first.state() == ComponentState.READY
        assert second.state() == ComponentState.READY
    finally:
        node.dispose()


@pytest.mark.asyncio
async def test_add_exec_algorithm_rejects_running_node():
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-008"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecEngineConfig(reconciliation=False),
            timeout_connection_secs=0,
            timeout_reconciliation_secs=0,
            timeout_portfolio_secs=0,
            timeout_disconnection_secs=0,
            delay_post_stop_secs=0,
            timeout_shutdown_secs=0,
        ),
    )
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-LIVE-RUNNING")),
    )

    handle = node.handle()
    task = asyncio.create_task(node.run_async())
    await asyncio.sleep(0.1)

    try:
        # `run_async` owns the node, so registration is refused before the running check.
        with pytest.raises(RuntimeError) as exc_info:
            node.add_exec_algorithm(exec_algorithm)
    finally:
        handle.stop()
        async with asyncio.timeout(10.0):
            await task

    assert "run_async" in str(exc_info.value)
    assert exec_algorithm.is_registered() is False
    assert handle.is_running is False


def test_add_exec_algorithm_rejects_data_actor_instance(live_node):
    with pytest.raises(
        RuntimeError,
        match="requires a Python v2 ExecutionAlgorithm instance",
    ):
        live_node.add_exec_algorithm(DataActor())


def test_add_exec_algorithm_from_config_rejects_invalid_path(live_node):
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="invalid_path_no_colon",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="exec_algorithm_path must be in format"):
        live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_from_config_rejects_nonexistent_module(live_node):
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_exec_algorithm_from_config(config)
