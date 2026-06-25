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

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.trading import ExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig


class RequiredConfigBacktestExecAlgorithmConfig(DataActorConfig):
    def __init__(
        self,
        exec_algorithm_id: str,
        actor_id=None,
        log_events: bool = True,
        log_commands: bool = True,
    ):
        self.actor_id = actor_id
        self.exec_algorithm_id = exec_algorithm_id
        self.log_events = log_events
        self.log_commands = log_commands


class RequiredConfigBacktestExecAlgorithm(DataActor):
    received_exec_algorithm_id: str | None = None

    def __init__(self, config: RequiredConfigBacktestExecAlgorithmConfig):
        super().__init__()
        type(self).received_exec_algorithm_id = config.exec_algorithm_id


def test_add_native_exec_algorithm_rejects_unknown_type():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP-UNKNOWN-TYPE"))

    with pytest.raises(TypeError, match="Unsupported native exec algorithm type: VwapAlgorithm"):
        engine.add_native_exec_algorithm("VwapAlgorithm", config)

    engine.dispose()


def test_add_native_exec_algorithm_requires_exec_algorithm_id():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))

    with pytest.raises(ValueError, match="TwapAlgorithm config requires `exec_algorithm_id`"):
        engine.add_native_exec_algorithm("TwapAlgorithm", ExecutionAlgorithmConfig())

    engine.dispose()


def test_add_native_exec_algorithm_rejects_duplicate_registration():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("TWAP-DUPLICATE"))
    engine.add_native_exec_algorithm("TwapAlgorithm", config)

    with pytest.raises(RuntimeError, match="'TWAP-DUPLICATE' is already registered"):
        engine.add_native_exec_algorithm("TwapAlgorithm", config)

    engine.dispose()


def test_add_exec_algorithm_from_config_registers_importable_algorithm():
    RequiredConfigBacktestExecAlgorithm.received_exec_algorithm_id = None
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithm"
        ),
        config_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithmConfig"
        ),
        config={"exec_algorithm_id": "BACKTEST-ALGO-CONFIG"},
    )

    engine.add_exec_algorithm_from_config(config)

    assert RequiredConfigBacktestExecAlgorithm.received_exec_algorithm_id == "BACKTEST-ALGO-CONFIG"
    engine.dispose()


def test_add_exec_algorithm_from_config_rejects_invalid_path():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="invalid_path_no_colon",
        config_path="module:Config",
        config={},
    )

    with pytest.raises(ValueError, match="exec_algorithm_path must be in format"):
        engine.add_exec_algorithm_from_config(config)

    engine.dispose()


def test_add_exec_algorithm_from_config_rejects_nonexistent_module():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="nonexistent.module:SomeClass",
        config_path="nonexistent.module:SomeConfig",
        config={},
    )

    with pytest.raises(RuntimeError, match="Failed to import module"):
        engine.add_exec_algorithm_from_config(config)

    engine.dispose()


def test_add_exec_algorithm_from_config_rejects_duplicate_registration():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path="tests.unit.common.actor:TestExecAlgorithm",
        config_path="tests.unit.common.actor:TestExecAlgorithmConfig",
        config={"actor_id": "BACKTEST-ALGO-DUPLICATE"},
    )
    engine.add_exec_algorithm_from_config(config)

    with pytest.raises(RuntimeError, match="'BACKTEST-ALGO-DUPLICATE' is already registered"):
        engine.add_exec_algorithm_from_config(config)

    engine.dispose()


def test_add_exec_algorithm_from_config_rejects_running_engine():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithm"
        ),
        config_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithmConfig"
        ),
        config={"exec_algorithm_id": "BACKTEST-ALGO-RUNNING"},
    )

    try:
        engine.run(streaming=True)
        with pytest.raises(RuntimeError, match="Cannot add execution algorithms to running trader"):
            engine.add_exec_algorithm_from_config(config)
    finally:
        engine.dispose()


def test_add_exec_algorithm_from_config_rejects_disposed_engine():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    config = ImportableExecAlgorithmConfig(
        exec_algorithm_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithm"
        ),
        config_path=(
            "tests.unit.backtest.test_backtest_engine_exec_algorithms:"
            "RequiredConfigBacktestExecAlgorithmConfig"
        ),
        config={"exec_algorithm_id": "BACKTEST-ALGO-DISPOSED"},
    )

    engine.run()
    engine.dispose()

    with pytest.raises(RuntimeError, match="Cannot add components to disposed trader"):
        engine.add_exec_algorithm_from_config(config)


def test_add_exec_algorithms_from_configs_registers_multiple_algorithms():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    configs = [
        ImportableExecAlgorithmConfig(
            exec_algorithm_path="tests.unit.common.actor:TestExecAlgorithm",
            config_path="tests.unit.common.actor:TestExecAlgorithmConfig",
            config={"actor_id": "BACKTEST-ALGO-A"},
        ),
        ImportableExecAlgorithmConfig(
            exec_algorithm_path="tests.unit.common.actor:TestExecAlgorithm",
            config_path="tests.unit.common.actor:TestExecAlgorithmConfig",
            config={"actor_id": "BACKTEST-ALGO-B"},
        ),
    ]

    engine.add_exec_algorithms_from_configs(configs)

    with pytest.raises(RuntimeError, match="'BACKTEST-ALGO-A' is already registered"):
        engine.add_exec_algorithm_from_config(configs[0])
    with pytest.raises(RuntimeError, match="'BACKTEST-ALGO-B' is already registered"):
        engine.add_exec_algorithm_from_config(configs[1])
    engine.dispose()
