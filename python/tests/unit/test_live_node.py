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
from nautilus_trader.common import Environment
from nautilus_trader.common import ImportableActorConfig
from nautilus_trader.live import LiveDataEngineConfig
from nautilus_trader.live import LiveExecEngineConfig
from nautilus_trader.live import LiveNode
from nautilus_trader.live import LiveRiskEngineConfig
from nautilus_trader.live import PortfolioConfig
from nautilus_trader.model import TraderId
from nautilus_trader.trading import ImportableExecAlgorithmConfig
from nautilus_trader.trading import ImportableStrategyConfig


@pytest.fixture(scope="module")
def live_node():
    trader_id = TraderId("TESTER-001")
    return LiveNode.builder("TEST", trader_id, Environment.SANDBOX).build()


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
