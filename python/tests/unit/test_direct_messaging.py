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
Exercise component topic messaging through the Python runtime.
"""

import gc
import subprocess
import sys
import textwrap
import weakref

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.common import DataActor
from nautilus_trader.config import BacktestEngineConfig
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import Strategy


INSTRUMENT_ID = InstrumentId.from_str("CUSTOM.TEST")
DATA_TYPE = DataType("RustTestCustomData", {"source": "topics"}, str(INSTRUMENT_ID))


class TopicCallbacks:
    """
    Subscribe during startup and record delivered objects.
    """

    def on_start(self) -> None:
        """
        Subscribe before the runtime processes data.
        """
        self.received = []
        self.subscribe_topic("app.shared", self.receive)

    def receive(self, message: object) -> None:
        """
        Retain the exact object delivered by the runtime.
        """
        self.received.append(message)

    def on_stop(self) -> None:
        """
        Keep the lifecycle hook free of side effects.
        """

    def on_resume(self) -> None:
        """
        Keep the lifecycle hook free of side effects.
        """

    def on_reset(self) -> None:
        """
        Keep the lifecycle hook free of side effects.
        """

    def on_dispose(self) -> None:
        """
        Keep the lifecycle hook free of side effects.
        """

    def on_fault(self) -> None:
        """
        Keep the lifecycle hook free of side effects.
        """


class TopicActor(TopicCallbacks, DataActor):
    """
    Receive application messages as an actor.
    """


class TopicStrategy(TopicCallbacks, Strategy):
    """
    Receive application messages as a strategy.
    """


class TopicAlgorithm(TopicCallbacks, ExecutionAlgorithm):
    """
    Receive application messages as an execution algorithm.
    """


class DataPublisher(TopicActor):
    """
    Publish application messages from real data callbacks.
    """

    def on_start(self) -> None:
        """
        Subscribe before the runtime processes data.
        """
        super().on_start()
        self.published = []
        self.subscribe_data(DATA_TYPE)

    def on_data(self, data: object) -> None:
        """
        Relay custom data through the application topic.
        """
        message = {"value": data.data.value, "flag": data.data.flag}
        self.published.append(message)
        self.publish_message("app.shared", message)


@pytest.fixture(name="engine")
def fixture_engine() -> object:
    """
    Provide a runtime with deterministic disposal.
    """
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    yield engine
    engine.dispose()


@pytest.fixture(name="components")
def fixture_components(engine) -> object:
    """
    Start all three component types on the same runtime.
    """
    components = (TopicActor(), TopicStrategy(), TopicAlgorithm())
    engine.add_actor(components[0])
    engine.add_strategy(components[1])
    engine.add_exec_algorithm(components[2])
    engine.run(streaming=True)
    return components


def test_data_callback_publishes_original_objects_to_all_components(engine) -> None:
    """
    Test data callback publishes original objects to all components.
    """
    register_custom_data_class(RustTestCustomData)
    actor = DataPublisher()
    strategy = TopicStrategy()
    algorithm = TopicAlgorithm()
    engine.add_actor(actor)
    engine.add_strategy(strategy)
    engine.add_exec_algorithm(algorithm)
    engine.add_data(
        [
            CustomData(DATA_TYPE, RustTestCustomData(INSTRUMENT_ID, 1.25, True, 1, 1)),
            CustomData(DATA_TYPE, RustTestCustomData(INSTRUMENT_ID, 2.5, False, 2, 2)),
        ],
        validate=True,
        sort=True,
    )

    engine.run()

    assert actor.published == [{"value": 1.25, "flag": True}, {"value": 2.5, "flag": False}]
    for component in (actor, strategy, algorithm):
        assert component.received == actor.published
        assert all(
            actual is expected
            for actual, expected in zip(component.received, actor.published, strict=True)
        )


@pytest.mark.parametrize("index", [0, 1, 2])
def test_each_component_publishes_and_unsubscribes_bound_method(components, index) -> None:
    """
    Test each component publishes and unsubscribes bound method.
    """
    publisher = components[index]
    first = {"sequence": 1}
    second = {"sequence": 2}

    publisher.publish_message("app.shared", first)
    publisher.unsubscribe_topic("app.shared", publisher.receive)
    publisher.publish_message("app.shared", second)

    for component in components:
        expected = [first] if component is publisher else [first, second]
        assert component.received == expected
        assert all(
            actual is original
            for actual, original in zip(component.received, expected, strict=True)
        )


@pytest.mark.parametrize("index", [0, 1, 2])
def test_duplicate_subscription_and_component_owned_unsubscribe(components, index) -> None:
    """
    Test duplicate subscription and component owned unsubscribe.
    """
    owner = components[index]
    peer = components[(index + 1) % 3]
    received = []
    handler = received.append
    owner.subscribe_topic("app.owned", handler, priority=10)
    owner.subscribe_topic("app.owned", handler, priority=1)
    peer.subscribe_topic("app.owned", handler)

    owner.publish_message("app.owned", 1)
    owner.unsubscribe_topic("app.owned", handler)
    owner.publish_message("app.owned", 2)
    peer.unsubscribe_topic("app.owned", handler)
    owner.publish_message("app.owned", 3)

    assert received == [1, 1, 2]


def test_wildcards_priority_and_synchronous_nested_publication(components) -> None:
    """
    Test wildcards priority and synchronous nested publication.
    """
    actor, strategy, algorithm = components
    calls = []
    message = {"sequence": 7}

    def high(payload) -> None:
        calls.append(("high", payload))
        actor.publish_message("app.nested", payload)
        calls.append(("returned", payload))

    actor.subscribe_topic("app.outer", high, priority=50)
    actor.subscribe_topic("app.outer", high, priority=0)
    strategy.subscribe_topic(
        "app.*",
        lambda payload: calls.append(("wildcard", payload)),
        priority=1,
    )
    algorithm.subscribe_topic(
        "app.nested",
        lambda payload: calls.append(("nested", payload)),
        priority=20,
    )
    algorithm.subscribe_topic(
        "app.outer",
        lambda payload: calls.append(("middle", payload)),
        priority=20,
    )

    actor.publish_message("app.outer", message)

    assert [name for name, _ in calls] == [
        "high",
        "nested",
        "wildcard",
        "returned",
        "middle",
        "wildcard",
    ]
    assert all(payload is message for _, payload in calls)


@pytest.mark.parametrize("index", [0, 1, 2])
@pytest.mark.parametrize("cleanup", ["reset", "dispose", "fault"])
def test_stop_resume_retains_subscription_and_cleanup_removes_it(
    components,
    index,
    cleanup,
) -> None:
    """
    Test stop resume retains subscription and cleanup removes it.
    """
    component = components[index]
    publisher = components[(index + 1) % 3]
    component.stop()
    publisher.publish_message("app.shared", "stopped")
    component.resume()
    publisher.publish_message("app.shared", "resumed")
    component.stop()
    getattr(component, cleanup)()
    publisher.publish_message("app.shared", "cleared")

    assert component.received == ["stopped", "resumed"]
    assert publisher.received == ["stopped", "resumed", "cleared"]


@pytest.mark.parametrize(
    ("component_type", "add", "clear"),
    [
        (TopicActor, "add_actor", "clear_actors"),
        (TopicStrategy, "add_strategy", "clear_strategies"),
        (TopicAlgorithm, "add_exec_algorithm", "clear_exec_algorithms"),
    ],
)
def test_engine_retirement_releases_bound_handler(engine, component_type, add, clear) -> None:
    """
    Test engine retirement releases bound handler.
    """
    component = component_type()
    getattr(engine, add)(component)
    engine.run(streaming=True)
    engine.end()
    reference = weakref.ref(component)

    getattr(engine, clear)()
    with pytest.raises(RuntimeError):
        component.publish_message("app.shared", "retired")
    del component
    gc.collect()

    assert reference() is None


@pytest.mark.parametrize("component_type", [TopicActor, TopicStrategy, TopicAlgorithm])
@pytest.mark.parametrize("method", ["publish_message", "subscribe_topic", "unsubscribe_topic"])
def test_unregistered_component_rejects_topic_operations(component_type, method) -> None:
    """
    Test unregistered component rejects topic operations.
    """
    component = component_type()
    with pytest.raises(
        RuntimeError,
        match="Component must be registered on the calling runtime thread",
    ):
        getattr(component, method)("app.shared", lambda message: None)


def test_topic_operations_on_foreign_thread_raise_without_aborting() -> None:
    """
    Test topic operations on foreign thread raise without aborting.
    """
    code = textwrap.dedent(
        """\
        import threading
        from nautilus_trader.backtest import BacktestEngine
        from nautilus_trader.config import BacktestEngineConfig
        from nautilus_trader.common import DataActor
        from nautilus_trader.trading import Strategy, ExecutionAlgorithm

        engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
        components = (DataActor(), Strategy(), ExecutionAlgorithm())
        engine.add_actor(components[0])
        engine.add_strategy(components[1])
        engine.add_exec_algorithm(components[2])
        errors = []

        def invoke():
            for component in components:
                for method in ("publish_message", "subscribe_topic", "unsubscribe_topic"):
                    try:
                        getattr(component, method)("app.shared", lambda message: None)
                    except RuntimeError as e:
                        errors.append(str(e))

        thread = threading.Thread(target=invoke)
        thread.start()
        thread.join()
        engine.dispose()
        assert errors == ["Component must be registered on the calling runtime thread"] * 9, errors
        print("checked 9 thread errors")
        """,
    )
    result = subprocess.run(
        [sys.executable, "-c", code],
        capture_output=True,
        text=True,
        encoding="utf-8",
        check=False,
        timeout=30,
    )

    assert result.returncode == 0, result.stderr
    assert "checked 9 thread errors" in result.stdout.splitlines()
