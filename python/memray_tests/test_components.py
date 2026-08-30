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
Test Python component ownership across repeated registration cycles.
"""

import gc
import weakref

import pytest
from tests.unit.test_component_ownership import OwnershipActor
from tests.unit.test_component_ownership import OwnershipExecutionAlgorithm
from tests.unit.test_component_ownership import OwnershipStrategy

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig


pytest.importorskip("pytest_memray")

_CYCLES = 64


@pytest.fixture(name="engine", scope="module")
def fixture_engine() -> object:
    """
    Fixture engine initialized outside Memray's tracked test call.
    """
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    yield engine
    engine.dispose()


@pytest.fixture(scope="module", autouse=True)
def _warm_up_component_registration(engine: BacktestEngine) -> None:
    _assert_components_released(engine)


@pytest.mark.limit_leaks("32 KB")
def test_registered_python_components_are_released_each_cycle(engine: BacktestEngine) -> None:
    """
    Test actors, strategies, and execution algorithms do not accumulate.
    """
    for _ in range(_CYCLES):
        _assert_components_released(engine)


def _assert_components_released(engine: BacktestEngine) -> None:
    actor = OwnershipActor()
    strategy = OwnershipStrategy()
    exec_algorithm = OwnershipExecutionAlgorithm()

    engine.add_actor(actor)
    engine.add_strategy(strategy)
    engine.add_exec_algorithm(exec_algorithm)

    sentinels = (
        weakref.ref(actor),
        weakref.ref(strategy),
        weakref.ref(exec_algorithm),
    )
    del actor, strategy, exec_algorithm

    engine.clear_actors()
    engine.clear_strategies()
    engine.clear_exec_algorithms()
    gc.collect()

    assert tuple(sentinel() for sentinel in sentinels) == (None, None, None)
