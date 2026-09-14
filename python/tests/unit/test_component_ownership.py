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
Test component ownership behavior.
"""

import gc
import weakref

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.model import ActorId
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import Strategy


class OwnershipActor(DataActor):
    """
    Collect ownership actor tests.
    """


class OwnershipStrategy(Strategy):
    """
    Collect ownership strategy tests.
    """


class OwnershipExecutionAlgorithm(ExecutionAlgorithm):
    """
    Collect ownership execution algorithm tests.
    """


class TimerOwnershipActor(DataActor):
    """
    Collect timer ownership actor tests.
    """

    def on_timer(self, event: object) -> None:
        """
        On timer.
        """


@pytest.fixture(name="engine")
def fixture_engine() -> object:
    """
    Fixture engine.
    """
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    yield engine
    engine.dispose()


@pytest.mark.parametrize(
    "component_type",
    [OwnershipActor, OwnershipStrategy, OwnershipExecutionAlgorithm],
)
def test_unregistered_component_is_collected(component_type: object) -> None:
    """
    Test unregistered component is collected.
    """
    component = component_type()
    sentinel = weakref.ref(component)

    del component
    gc.collect()

    # A strong `py_self` would form an untraceable cycle and keep this alive
    assert sentinel() is None


def test_registered_components_stay_alive_until_cleared(engine: object) -> None:
    """
    Test registered components stay alive until cleared.
    """
    actor = OwnershipActor()
    strategy = OwnershipStrategy()
    exec_algorithm = OwnershipExecutionAlgorithm()

    engine.add_actor(actor)
    engine.add_strategy(strategy)
    engine.add_exec_algorithm(exec_algorithm)

    sentinels = {
        "actor": weakref.ref(actor),
        "strategy": weakref.ref(strategy),
        "exec_algorithm": weakref.ref(exec_algorithm),
    }

    del actor, strategy, exec_algorithm
    gc.collect()

    assert {name: sentinel() is not None for name, sentinel in sentinels.items()} == {
        "actor": True,
        "strategy": True,
        "exec_algorithm": True,
    }

    engine.clear_actors()
    engine.clear_strategies()
    engine.clear_exec_algorithms()
    gc.collect()

    assert {name: sentinel() for name, sentinel in sentinels.items()} == {
        "actor": None,
        "strategy": None,
        "exec_algorithm": None,
    }


def test_registered_component_keeps_config_reachable_after_disposal(engine: object) -> None:
    """
    Test registered component keeps config reachable after disposal.
    """
    config = {"label": "ownership"}
    actor = OwnershipActor(config)

    engine.add_actor(actor)
    engine.clear_actors()

    assert actor.config == config


def test_registered_component_clock_callback_released_on_clear(engine: object) -> None:
    """
    Test registered component clock callback released on clear.
    """
    actor = TimerOwnershipActor()
    engine.add_actor(actor)

    # A bound method holds the wrapper, so the clock owns the actor until retirement
    actor.clock.set_time_alert_ns(
        "ownership-alert",
        actor.clock.timestamp_ns() + 60_000_000_000,
        callback=actor.on_timer,
    )
    sentinel = weakref.ref(actor)

    del actor
    gc.collect()

    assert sentinel() is not None

    engine.clear_actors()
    gc.collect()

    assert sentinel() is None


INSTRUMENT_ID = InstrumentId.from_str("CUSTOM.TEST")
DATA_TYPE = DataType("RustTestCustomData", {"source": "ownership"}, str(INSTRUMENT_ID))


class SubscribingActor(DataActor):
    """
    Collect subscribing actor tests.
    """

    def __init__(self, config: object = None) -> None:
        """
        Initialize the instance.
        """
        super().__init__(config)
        self.received = []

    def on_start(self) -> None:
        """
        On start.
        """
        self.subscribe_data(DATA_TYPE)

    def on_data(self, data: object) -> None:
        """
        On data.
        """
        self.received.append(data.data.value)


def _custom_data() -> object:
    return [
        CustomData(DATA_TYPE, RustTestCustomData(INSTRUMENT_ID, 1.25, True, 1, 1)),
        CustomData(DATA_TYPE, RustTestCustomData(INSTRUMENT_ID, 2.5, False, 2, 2)),
    ]


def test_actor_can_be_retired_and_replaced_between_runs(engine: object) -> None:
    """
    Retiring an actor and adding a replacement leaves a working engine.

    This pins the end-to-end lifecycle only. It does not prove that the retired actor's
    message bus handler was removed, because no non-destructive way to inspect the bus is
    exposed to Python: constructing a `MessageBus` replaces the thread-local one. Handler
    removal is pinned by the Rust `test_retirement_removes_component_subscriptions`.

    """
    register_custom_data_class(RustTestCustomData)

    retired = SubscribingActor(DataActorConfig(actor_id=ActorId("Retired-Subscriber")))
    engine.add_actor(retired)
    engine.add_data(_custom_data(), validate=True, sort=True)
    engine.run()

    assert retired.received == [1.25, 2.5]

    engine.reset()
    engine.clear_actors()

    survivor = SubscribingActor(DataActorConfig(actor_id=ActorId("Later-Subscriber")))
    engine.add_actor(survivor)
    engine.run()

    assert survivor.received == [1.25, 2.5]
    assert retired.received == [1.25, 2.5], "a retired actor must receive no further data"


def test_repeated_registration_cycles_do_not_accumulate(engine: object) -> None:
    """
    Test repeated registration cycles do not accumulate.
    """
    sentinels = []

    for cycle in range(25):
        actor = OwnershipActor(DataActorConfig(actor_id=ActorId(f"Cycle-Actor-{cycle}")))
        strategy = OwnershipStrategy()
        exec_algorithm = OwnershipExecutionAlgorithm()

        engine.add_actor(actor)
        engine.add_strategy(strategy)
        engine.add_exec_algorithm(exec_algorithm)

        sentinels.extend(
            [weakref.ref(actor), weakref.ref(strategy), weakref.ref(exec_algorithm)],
        )
        del actor, strategy, exec_algorithm

        engine.clear_actors()
        engine.clear_strategies()
        engine.clear_exec_algorithms()

    gc.collect()

    assert [sentinel() for sentinel in sentinels] == [None] * len(sentinels)
