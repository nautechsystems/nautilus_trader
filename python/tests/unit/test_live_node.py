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

import pytest

from nautilus_trader.common import CacheConfig
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
from nautilus_trader.model import TraderId
from nautilus_trader.trading import ImportableControllerConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig
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


@pytest.mark.parametrize(
    ("trader_id", "stop_before_dispose"),
    [
        ("TESTER-004", True),
        ("TESTER-005", False),
    ],
)
def test_live_node_start_stop_dispose_local(trader_id, stop_before_dispose):
    LifecycleProbeStrategy.reset()
    node = LiveNode.build(
        "TEST",
        LiveNodeConfig(
            trader_id=TraderId(trader_id),
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

    try:
        assert node.is_running is False

        node.start()
        assert node.is_running is True

        if stop_before_dispose:
            node.stop()
        else:
            node.dispose()

        assert node.is_running is False
    finally:
        node.dispose()
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


def test_live_node_start_after_dispose_raises():
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
        with pytest.raises(RuntimeError, match="Invalid state trigger DISPOSED -> START"):
            node.start()
    finally:
        node.dispose()


def test_live_node_strategy_start_failure_disposes_resources():
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

    with pytest.raises(RuntimeError, match="simulated live node strategy start failure"):
        node.start()

    assert node.is_running is False

    node.dispose()
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
