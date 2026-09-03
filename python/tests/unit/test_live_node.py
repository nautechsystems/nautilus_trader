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
"""
Test live node behavior.
"""

import asyncio
from typing import ClassVar

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
from nautilus_trader.live import LiveExecutionEngineConfig
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
from nautilus_trader.trading import ImportableExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from tests.unit.common.actor import ControllerRegistrationProbe
from tests.unit.common.actor import LifecycleProbeStrategy


@pytest.fixture(scope="module")
def live_node() -> object:
    """
    Live node.
    """
    trader_id = TraderId("TESTER-001")
    return LiveNode.builder("TEST", trader_id, Environment.SANDBOX).build()


class RequiredConfigLiveActorConfig(DataActorConfig):
    """
    Collect required config live actor config tests.
    """

    def __init__(
        self,
        required_label: str,
        actor_id: object = None,
        log_events: bool = True,
        log_commands: bool = True,
    ) -> None:
        """
        Initialize the instance.
        """
        self.actor_id = actor_id
        self.log_events = log_events
        self.log_commands = log_commands
        self.required_label = required_label


class RequiredConfigLiveActor(DataActor):
    """
    Collect required config live actor tests.
    """

    received_actor_id: str | None = None
    received_label: str | None = None

    def __init__(self, config: RequiredConfigLiveActorConfig) -> None:
        """
        Initialize the instance.
        """
        super().__init__()
        type(self).received_actor_id = str(config.actor_id)
        type(self).received_label = config.required_label


class LifecycleExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect lifecycle execution algorithm tests.
    """

    start_observations: ClassVar[list[object]] = []

    def on_start(self) -> None:
        """
        On start.
        """
        type(self).start_observations.append((self.state, self.is_running()))


class FirstDefaultLiveExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect first default live execution algorithm tests.
    """


class SecondDefaultLiveExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect second default live execution algorithm tests.
    """


class FirstDefaultLiveStrategy(Strategy):
    """
    Collect first default live strategy tests.
    """


class SecondDefaultLiveStrategy(Strategy):
    """
    Collect second default live strategy tests.
    """


class DefaultIdLiveStrategy(Strategy):
    """
    Collect default id live strategy tests.
    """

    instances: ClassVar[list[object]] = []

    def __init__(self, config: StrategyConfig | None = None) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        type(self).instances.append(self)


class DefaultIdLiveActor(DataActor):
    """
    Collect default id live actor tests.
    """

    instances: ClassVar[list[object]] = []

    def __init__(self, config: DataActorConfig | None = None) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        type(self).instances.append(self)


class SecondDefaultIdLiveActor(DefaultIdLiveActor):
    """
    Collect second default id live actor tests.
    """


class NonForwardingLiveActor(DataActor):
    """
    Collect non forwarding live actor tests.
    """

    instances: ClassVar[list[object]] = []

    def __init__(self, _config: object = None) -> None:
        """
        Initialize the instance.
        """
        # Deliberately does not forward to `super().__init__()`
        type(self).instances.append(self)


class SecondNonForwardingLiveActor(NonForwardingLiveActor):
    """
    Collect second non forwarding live actor tests.
    """


class InternalConfigLiveActor(DataActor):
    """
    Collect internal config live actor tests.
    """

    instances: ClassVar[list[object]] = []

    def __init__(self) -> None:
        """
        Initialize the instance.
        """
        super().__init__(DataActorConfig(actor_id=ActorId("INTERNAL-CONFIG-ACTOR")))
        type(self).instances.append(self)


def test_importable_actor_config_construction() -> None:
    """
    Test importable actor config construction.
    """
    config = ImportableActorConfig(
        actor_path="tests.unit.common.actor:TestActor",
        config_path="tests.unit.common.actor:TestActorConfig",
        config={"actor_id": "TEST-001"},
    )

    assert config.actor_path == "tests.unit.common.actor:TestActor"
    assert config.config_path == "tests.unit.common.actor:TestActorConfig"
    assert config.config == {"actor_id": "TEST-001"}


def test_importable_actor_config_empty() -> None:
    """
    Test importable actor config empty.
    """
    config = ImportableActorConfig(
        actor_path="module:Class",
        config_path="module:Config",
        config={},
    )

    assert config.actor_path == "module:Class"
    assert config.config == {}


def test_importable_strategy_config_construction() -> None:
    """
    Test importable strategy config construction.
    """
    config = ImportableStrategyConfig(
        strategy_path="tests.unit.common.actor:TestStrategy",
        config_path="nautilus_trader.trading:StrategyConfig",
        config={"strategy_id": "S-001"},
    )

    assert config.strategy_path == "tests.unit.common.actor:TestStrategy"
    assert config.config_path == "nautilus_trader.trading:StrategyConfig"
    assert config.config == {"strategy_id": "S-001"}


def test_importable_controller_config_construction() -> None:
    """
    Test importable controller config construction.
    """
    config = ImportableControllerConfig(
        controller_path="tests.unit.common.actor:StrategyCreatingController",
        config_path="tests.unit.common.actor:TestControllerConfig",
        config={"actor_id": "Controller-001"},
    )

    assert config.controller_path == "tests.unit.common.actor:StrategyCreatingController"
    assert config.config_path == "tests.unit.common.actor:TestControllerConfig"
    assert config.config == {"actor_id": "Controller-001"}


def test_live_node_config_registers_importable_controller() -> None:
    """
    Test live node config registers importable controller.
    """
    ControllerRegistrationProbe.reset()
    trader_id = TraderId("TESTER-003")
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=trader_id,
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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


def test_live_node_exposes_cache_and_portfolio_inspection() -> None:
    """
    Test live node exposes cache and portfolio inspection.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-010"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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
async def test_live_node_start_stop_dispose_local() -> None:
    """
    Test live node start stop dispose local.
    """
    LifecycleProbeStrategy.reset()
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-004"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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


def test_live_node_dispose_before_start_twice_does_not_raise() -> None:
    """
    Test live node dispose before start twice does not raise.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-006"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
        ),
    )

    node.dispose()
    node.dispose()


def test_live_node_stop_before_start_raises() -> None:
    """
    Test live node stop before start raises.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-008"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
        ),
    )

    try:
        with pytest.raises(RuntimeError, match="LiveNode is not running"):
            node.stop()
    finally:
        node.dispose()


@pytest.mark.asyncio
async def test_live_node_run_after_dispose_raises() -> None:
    """
    Test live node run after dispose raises.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-009"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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
async def test_live_node_strategy_start_failure_disposes_resources() -> None:
    """
    Test live node strategy start failure disposes resources.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-007"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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


def test_importable_exec_algorithm_config_construction() -> None:
    """
    Test importable exec algorithm config construction.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="tests.unit.common.actor:TestExecutionAlgorithm",
        config_path="tests.unit.common.actor:TestExecutionAlgorithmConfig",
        config={"actor_id": "ALGO-001"},
    )

    assert config.exec_algorithm_path == "tests.unit.common.actor:TestExecutionAlgorithm"
    assert config.config_path == "tests.unit.common.actor:TestExecutionAlgorithmConfig"
    assert config.config == {"actor_id": "ALGO-001"}


def test_importable_exec_algorithm_config_empty() -> None:
    """
    Test importable exec algorithm config empty.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="module:Class",
        config_path="module:Config",
        config={},
    )

    assert config.exec_algorithm_path == "module:Class"
    assert config.config == {}


def test_builder_accepts_supported_runtime_configs() -> None:
    """
    Test builder accepts supported runtime configs.
    """
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
        .with_exec_engine_config(LiveExecutionEngineConfig(reconciliation=False))
        .build()
    )

    assert node.trader_id == trader_id
    assert node.environment == Environment.SANDBOX


def test_builder_rejects_unregistered_cache_database_factory() -> None:
    """
    Test builder rejects unregistered cache database factory.
    """
    builder = LiveNode.builder("TEST", TraderId("TESTER-003"), Environment.SANDBOX)

    with pytest.raises(
        NotImplementedError,
        match="No cache database factory extractor registered for 'dict'",
    ):
        builder.with_cache_database_factory({})


def test_add_actor_from_config_registers(live_node: LiveNode) -> None:
    """
    Test add actor from config registers.
    """
    config = ImportableActorConfig(
        actor_path="tests.unit.common.actor:TestActor",
        config_path="tests.unit.common.actor:TestActorConfig",
        config={},
    )

    live_node.add_actor_from_config(config)


def test_add_actor_from_config_accepts_required_subclass_kwargs(live_node: LiveNode) -> None:
    """
    Test add actor from config accepts required subclass kwargs.
    """
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


def test_add_actor_from_config_rejects_invalid_path(live_node: LiveNode) -> None:
    """
    Test add actor from config rejects invalid path.
    """
    config = ImportableActorConfig(
        actor_path="no_colon_here",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="actor_path must be in format"):
        live_node.add_actor_from_config(config)


def test_add_actor_from_config_rejects_nonexistent_module(live_node: LiveNode) -> None:
    """
    Test add actor from config rejects nonexistent module.
    """
    config = ImportableActorConfig(
        actor_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_actor_from_config(config)


def test_add_actor_from_empty_config_preserves_class_derived_id(live_node: LiveNode) -> None:
    """
    Test add actor from empty config preserves class derived ID.
    """
    InternalConfigLiveActor.instances.clear()
    config = ImportableActorConfig(
        actor_path="tests.unit.test_live_node:InternalConfigLiveActor",
        config_path="",
        config={},
    )

    live_node.add_actor_from_config(config)

    actor = InternalConfigLiveActor.instances[-1]
    assert actor.config.actor_id == ActorId("INTERNAL-CONFIG-ACTOR")
    assert actor.actor_id == ActorId("InternalConfigLiveActor")
    assert actor.state() == ComponentState.READY


def test_add_actor_registers_constructed_instance(live_node: LiveNode) -> None:
    """
    Test add actor registers constructed instance.
    """
    actor_id = ActorId("PY-LIVE-CONSTRUCTED-ACTOR")
    actor = DataActor(
        DataActorConfig(
            actor_id=actor_id,
            log_events=False,
            log_commands=False,
        ),
    )

    live_node.add_actor(actor)

    assert actor.actor_id == actor_id
    assert actor.trader_id == live_node.trader_id
    assert actor.state() == ComponentState.READY
    assert actor.is_ready() is True

    duplicate = DataActor(DataActorConfig(actor_id=actor_id))

    with pytest.raises(RuntimeError) as exc_info:
        live_node.add_actor(duplicate)

    assert str(exc_info.value) == "Actor 'PY-LIVE-CONSTRUCTED-ACTOR' is already registered"
    assert duplicate.trader_id is None
    assert duplicate.state() == ComponentState.PRE_INITIALIZED
    assert duplicate.is_ready() is False


def test_add_strategy_from_config_registers(live_node: LiveNode) -> None:
    """
    Test add strategy from config registers.
    """
    config = ImportableStrategyConfig(
        strategy_path="tests.unit.common.actor:TestStrategy",
        config_path="nautilus_trader.trading:StrategyConfig",
        config={},
    )

    live_node.add_strategy_from_config(config)


def test_add_strategy_from_config_rejects_invalid_path(live_node: LiveNode) -> None:
    """
    Test add strategy from config rejects invalid path.
    """
    config = ImportableStrategyConfig(
        strategy_path="no_colon_here",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="strategy_path must be in format"):
        live_node.add_strategy_from_config(config)


def test_add_strategy_from_config_rejects_nonexistent_module(live_node: LiveNode) -> None:
    """
    Test add strategy from config rejects nonexistent module.
    """
    config = ImportableStrategyConfig(
        strategy_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_strategy_from_config(config)


def test_add_exec_algorithm_from_config_registers(live_node: LiveNode) -> None:
    """
    Test add exec algorithm from config registers.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="tests.unit.common.actor:TestExecutionAlgorithm",
        config_path="tests.unit.common.actor:TestExecutionAlgorithmConfig",
        config={},
    )

    live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_from_config_registers_v2_instance(live_node: LiveNode) -> None:
    """
    Test add exec algorithm from config registers v2 instance.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="tests.unit.test_live_node:LifecycleExecutionAlgorithm",
        config_path="nautilus_trader.trading:ExecutionAlgorithmConfig",
        config={"exec_algorithm_id": "PY-LIVE-CONFIG"},
    )

    live_node.add_exec_algorithm_from_config(config)

    with pytest.raises(RuntimeError, match="'PY-LIVE-CONFIG' is already registered"):
        live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_registers_constructed_instance(live_node: LiveNode) -> None:
    """
    Test add exec algorithm registers constructed instance.
    """
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


def test_add_exec_algorithms_registers_distinct_class_derived_ids(live_node: LiveNode) -> None:
    """
    Test add exec algorithms registers distinct class derived ids.
    """
    first = FirstDefaultLiveExecutionAlgorithm()
    second = SecondDefaultLiveExecutionAlgorithm()

    live_node.add_exec_algorithm(first)
    live_node.add_exec_algorithm(second)

    assert first.exec_algorithm_id == ExecAlgorithmId("FirstDefaultLiveExecutionAlgorithm")
    assert first.is_registered() is True
    assert second.exec_algorithm_id == ExecAlgorithmId("SecondDefaultLiveExecutionAlgorithm")
    assert second.is_registered() is True


def test_add_strategies_registers_distinct_class_derived_ids() -> None:
    """
    Test add strategies registers distinct class derived ids.
    """
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


def test_add_strategy_from_config_derives_class_derived_id() -> None:
    """
    Test add strategy from config derives class derived id.
    """
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


def test_add_actors_from_config_derive_distinct_class_derived_ids() -> None:
    """
    Test add actors from config derive distinct class derived ids.
    """
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


def test_add_actors_from_config_derive_ids_without_a_forwarding_constructor() -> None:
    """
    Test add actors from config derive ids without a forwarding constructor.
    """
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
async def test_add_exec_algorithm_rejects_running_node() -> None:
    """
    Test add exec algorithm rejects running node.
    """
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId("TESTER-008"),
            environment=Environment.SANDBOX,
            exec_engine=LiveExecutionEngineConfig(reconciliation=False),
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


def test_add_exec_algorithm_rejects_data_actor_instance(live_node: LiveNode) -> None:
    """
    Test add exec algorithm rejects data actor instance.
    """
    with pytest.raises(
        RuntimeError,
        match="requires a Python v2 ExecutionAlgorithm instance",
    ):
        live_node.add_exec_algorithm(DataActor())


def test_add_exec_algorithm_from_config_rejects_invalid_path(live_node: LiveNode) -> None:
    """
    Test add exec algorithm from config rejects invalid path.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="invalid_path_no_colon",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="exec_algorithm_path must be in format"):
        live_node.add_exec_algorithm_from_config(config)


def test_add_exec_algorithm_from_config_rejects_nonexistent_module(live_node: LiveNode) -> None:
    """
    Test add exec algorithm from config rejects nonexistent module.
    """
    config = ImportableExecutionAlgorithmConfig(
        exec_algorithm_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        live_node.add_exec_algorithm_from_config(config)
