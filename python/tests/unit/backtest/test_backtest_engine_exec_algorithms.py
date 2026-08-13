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

import inspect

import pytest

from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import ComponentState
from nautilus_trader.common import DataActor
from nautilus_trader.common import DataActorConfig
from nautilus_trader.core import UUID4
from nautilus_trader.model import ActorId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import ExecAlgorithmId
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TraderId
from nautilus_trader.trading import ExecutionAlgorithm
from nautilus_trader.trading import ExecutionAlgorithmConfig
from nautilus_trader.trading import ImportableExecAlgorithmConfig


DATA_PUBLISHING_REGISTRATION_ERROR = "ExecutionAlgorithm must be registered before publishing data"


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


class CustomExecutionAlgorithmConfig(ExecutionAlgorithmConfig):
    def __init__(
        self,
        horizon_secs: str,
        interval_secs: str,
        **_kwargs,
    ):
        self.horizon_secs = horizon_secs
        self.interval_secs = interval_secs


class CustomExecutionAlgorithm(ExecutionAlgorithm):
    received_config: CustomExecutionAlgorithmConfig | None = None

    def __init__(self, config: CustomExecutionAlgorithmConfig):
        super().__init__(config)
        type(self).received_config = config


class FirstDefaultExecutionAlgorithm(ExecutionAlgorithm):
    pass


class SecondDefaultExecutionAlgorithm(ExecutionAlgorithm):
    pass


def _custom_data():
    class Payload:
        ts_event = 3
        ts_init = 4

    return CustomData(DataType("Payload"), Payload())


def _data_publishing_registration_cases():
    custom_data = _custom_data()

    return [
        ("publish_data", (custom_data.data_type, custom_data)),
        ("publish_signal", ("risk", "value")),
    ]


class NonForwardingExecutionAlgorithm(ExecutionAlgorithm):
    def __init__(self, config):
        super().__init__()


class InternalConfigExecutionAlgorithm(ExecutionAlgorithm):
    def __init__(self):
        super().__init__(
            ExecutionAlgorithmConfig(
                exec_algorithm_id=ExecAlgorithmId("INTERNAL-CONFIG"),
            ),
        )


class InternalActorIdExecutionAlgorithm(ExecutionAlgorithm):
    def __init__(self):
        super().__init__(
            DataActorConfig(
                actor_id=ActorId("INTERNAL-ACTOR-ID"),
            ),
        )


@pytest.mark.parametrize(
    ("method_name", "parameter_names"),
    [
        ("execute", ["self", "command"]),
        ("on_order", ["self", "order"]),
        ("on_order_list", ["self", "order_list", "orders"]),
        ("on_order_event", ["self", "event"]),
        ("on_order_initialized", ["self", "event"]),
        ("on_order_denied", ["self", "event"]),
        ("on_order_emulated", ["self", "event"]),
        ("on_order_released", ["self", "event"]),
        ("on_order_submitted", ["self", "event"]),
        ("on_order_rejected", ["self", "event"]),
        ("on_order_accepted", ["self", "event"]),
        ("on_order_canceled", ["self", "event"]),
        ("on_order_expired", ["self", "event"]),
        ("on_order_triggered", ["self", "event"]),
        ("on_order_pending_update", ["self", "event"]),
        ("on_order_pending_cancel", ["self", "event"]),
        ("on_order_modify_rejected", ["self", "event"]),
        ("on_order_cancel_rejected", ["self", "event"]),
        ("on_order_updated", ["self", "event"]),
        ("on_order_filled", ["self", "event"]),
        ("on_position_event", ["self", "event"]),
        ("on_position_opened", ["self", "event"]),
        ("on_position_changed", ["self", "event"]),
        ("on_position_closed", ["self", "event"]),
        (
            "spawn_market",
            [
                "self",
                "primary",
                "quantity",
                "time_in_force",
                "reduce_only",
                "tags",
                "reduce_primary",
            ],
        ),
        (
            "spawn_limit",
            [
                "self",
                "primary",
                "quantity",
                "price",
                "time_in_force",
                "expire_time",
                "post_only",
                "reduce_only",
                "display_qty",
                "emulation_trigger",
                "tags",
                "reduce_primary",
            ],
        ),
        (
            "spawn_market_to_limit",
            [
                "self",
                "primary",
                "quantity",
                "time_in_force",
                "expire_time",
                "reduce_only",
                "display_qty",
                "emulation_trigger",
                "tags",
                "reduce_primary",
            ],
        ),
        ("deny_order", ["self", "order", "reason"]),
        ("submit_order", ["self", "order", "position_id", "client_id"]),
        ("modify_order", ["self", "order", "quantity", "price", "trigger_price", "client_id"]),
        ("modify_order_in_place", ["self", "order", "quantity", "price", "trigger_price"]),
        ("cancel_order", ["self", "order", "client_id"]),
        ("publish_data", ["self", "data_type", "data"]),
        ("publish_signal", ["self", "name", "value", "ts_event"]),
        ("subscribe_signal", ["self", "name", "priority"]),
        ("subscribe_queue_state", ["self", "priority"]),
        ("subscribe_socket_state", ["self", "priority"]),
        ("unsubscribe_signal", ["self", "name"]),
        ("unsubscribe_queue_state", ["self"]),
        ("unsubscribe_socket_state", ["self"]),
        ("on_signal", ["self", "signal"]),
        ("on_queue_state", ["self", "event"]),
        ("on_socket_state", ["self", "event"]),
        ("to_importable_config", ["self"]),
        ("is_ready", ["self"]),
        ("is_running", ["self"]),
        ("is_stopped", ["self"]),
        ("is_disposed", ["self"]),
        ("is_degraded", ["self"]),
        ("is_faulted", ["self"]),
        ("start", ["self"]),
        ("stop", ["self"]),
        ("resume", ["self"]),
        ("reset", ["self"]),
        ("dispose", ["self"]),
        ("degrade", ["self"]),
        ("fault", ["self"]),
    ],
)
def test_execution_algorithm_authoring_surface_parameters(method_name, parameter_names):
    method = getattr(ExecutionAlgorithm, method_name)

    assert list(inspect.signature(method).parameters) == parameter_names


@pytest.mark.parametrize("method_name", ["subscribe_queue_state", "subscribe_socket_state"])
def test_execution_algorithm_state_subscription_priority_defaults_to_none(method_name):
    signature = inspect.signature(getattr(ExecutionAlgorithm, method_name))

    assert signature.parameters["priority"].default is None


@pytest.mark.parametrize(
    "attribute",
    [
        "greeks",
        "msgbus",
        "registered_indicators",
        "register_indicator",
        "subscribe_data",
        "unsubscribe_data",
        "on_data",
        "subscribe_instruments",
        "subscribe_quotes",
        "subscribe_trades",
        "subscribe_bars",
        "register",
    ],
)
def test_execution_algorithm_keeps_routed_order_surface(attribute):
    assert not hasattr(ExecutionAlgorithm, attribute)


def test_execution_algorithm_pre_registration_surface():
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-PRE-REGISTRATION")),
    )

    assert exec_algorithm.state == ComponentState.PRE_INITIALIZED
    assert exec_algorithm.is_registered() is False
    assert exec_algorithm.is_ready() is False
    assert exec_algorithm.is_running() is False
    assert exec_algorithm.is_stopped() is False
    assert exec_algorithm.is_disposed() is False
    assert exec_algorithm.is_degraded() is False
    assert exec_algorithm.is_faulted() is False

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = exec_algorithm.portfolio

    with pytest.raises(RuntimeError, match="ExecutionAlgorithm not registered"):
        exec_algorithm.deny_order(
            create_market_order(),
            "VALIDATION_FAILED: invalid Python execution schedule",
        )


@pytest.mark.parametrize(
    ("method_name", "args"),
    [
        ("subscribe_signal", ("risk",)),
        ("subscribe_queue_state", ()),
        ("subscribe_socket_state", ()),
        ("unsubscribe_signal", ("risk",)),
        ("unsubscribe_queue_state", ()),
        ("unsubscribe_socket_state", ()),
    ],
)
def test_execution_algorithm_subscriptions_require_registration(method_name, args):
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-PRE-REGISTRATION")),
    )

    with pytest.raises(RuntimeError) as exc_info:
        getattr(exec_algorithm, method_name)(*args)

    assert (
        str(exc_info.value) == "ExecutionAlgorithm must be registered before managing subscriptions"
    )


@pytest.mark.parametrize(
    ("method_name", "args"),
    _data_publishing_registration_cases(),
)
def test_execution_algorithm_data_publishing_requires_registration(method_name, args):
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-PRE-REGISTRATION")),
    )

    with pytest.raises(RuntimeError) as exc_info:
        getattr(exec_algorithm, method_name)(*args)

    assert str(exc_info.value) == DATA_PUBLISHING_REGISTRATION_ERROR


def test_execution_algorithm_registration_precedes_publish_signal_conversion():
    class InvalidSignalValue:
        def __str__(self):
            raise ValueError("invalid signal value")

    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-PRE-REGISTRATION")),
    )

    with pytest.raises(RuntimeError) as exc_info:
        exec_algorithm.publish_signal("risk", InvalidSignalValue())

    assert str(exc_info.value) == DATA_PUBLISHING_REGISTRATION_ERROR


def test_execution_algorithm_data_publishing_succeeds_when_registered():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-PUBLISH")),
    )
    custom_data = _custom_data()
    engine.add_exec_algorithm(exec_algorithm)

    try:
        assert exec_algorithm.publish_data(custom_data.data_type, custom_data) is None
        assert exec_algorithm.publish_signal("risk", "value") is None
    finally:
        engine.dispose()


@pytest.mark.parametrize(
    "method_name",
    ["start", "stop", "resume", "reset", "dispose", "degrade", "fault"],
)
def test_execution_algorithm_lifecycle_methods_reject_pre_initialized_state(method_name):
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-LIFECYCLE")),
    )

    with pytest.raises(RuntimeError, match="Invalid state trigger PRE_INITIALIZED"):
        getattr(exec_algorithm, method_name)()


def test_execution_algorithm_config_defaults():
    config = ExecutionAlgorithmConfig()

    assert config.exec_algorithm_id is None
    assert config.log_events is True
    assert config.log_commands is True


def test_execution_algorithm_config_supports_custom_fields():
    config = CustomExecutionAlgorithmConfig(
        horizon_secs="73.5",
        interval_secs="2.25",
        exec_algorithm_id=ExecAlgorithmId("CUSTOM-CONFIG"),
        log_events=False,
        log_commands=True,
    )

    assert config.exec_algorithm_id == ExecAlgorithmId("CUSTOM-CONFIG")
    assert config.horizon_secs == "73.5"
    assert config.interval_secs == "2.25"
    assert config.log_events is False
    assert config.log_commands is True


def test_execution_algorithm_to_importable_config_round_trips_custom_config():
    CustomExecutionAlgorithm.received_config = None
    config = CustomExecutionAlgorithmConfig(
        horizon_secs="91.5",
        interval_secs="3.75",
        exec_algorithm_id=ExecAlgorithmId("IMPORTABLE-CONFIG"),
        log_events=False,
        log_commands=True,
    )
    exec_algorithm = CustomExecutionAlgorithm(config)

    importable = exec_algorithm.to_importable_config()

    assert importable.exec_algorithm_path == (
        "tests.unit.backtest.test_backtest_engine_exec_algorithms:CustomExecutionAlgorithm"
    )
    assert importable.config_path == (
        "tests.unit.backtest.test_backtest_engine_exec_algorithms:CustomExecutionAlgorithmConfig"
    )
    assert importable.config == {
        "exec_algorithm_id": "IMPORTABLE-CONFIG",
        "horizon_secs": "91.5",
        "interval_secs": "3.75",
        "log_commands": True,
        "log_events": False,
    }

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_exec_algorithm_from_config(importable)

    received = CustomExecutionAlgorithm.received_config
    assert received is not None
    assert received.exec_algorithm_id == ExecAlgorithmId("IMPORTABLE-CONFIG")
    assert received.horizon_secs == "91.5"
    assert received.interval_secs == "3.75"
    assert received.log_events is False
    assert received.log_commands is True
    engine.dispose()


def test_execution_algorithm_to_importable_config_round_trips_without_config():
    exec_algorithm = FirstDefaultExecutionAlgorithm()

    importable = exec_algorithm.to_importable_config()

    assert importable.exec_algorithm_path == (
        "tests.unit.backtest.test_backtest_engine_exec_algorithms:FirstDefaultExecutionAlgorithm"
    )
    assert importable.config_path == ""
    assert importable.config == {}

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_exec_algorithm_from_config(importable)

    with pytest.raises(
        RuntimeError,
        match="'FirstDefaultExecutionAlgorithm' is already registered",
    ):
        engine.add_exec_algorithm_from_config(importable)
    engine.dispose()


def test_execution_algorithm_config_with_explicit_values():
    config = ExecutionAlgorithmConfig(
        exec_algorithm_id=ExecAlgorithmId("TWAP-001"),
        log_events=False,
        log_commands=False,
    )

    assert config.exec_algorithm_id == ExecAlgorithmId("TWAP-001")
    assert config.log_events is False
    assert config.log_commands is False


def test_execution_algorithm_derives_default_id_from_runtime_class():
    base = ExecutionAlgorithm()
    first = FirstDefaultExecutionAlgorithm()
    second = SecondDefaultExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=None),
    )

    assert base.exec_algorithm_id == ExecAlgorithmId("ExecutionAlgorithm")
    assert first.exec_algorithm_id == ExecAlgorithmId("FirstDefaultExecutionAlgorithm")
    assert second.exec_algorithm_id == ExecAlgorithmId("SecondDefaultExecutionAlgorithm")


def test_execution_algorithm_preserves_explicit_id_without_forwarding_config():
    exec_algorithm_id = ExecAlgorithmId("NON-FORWARDING")
    exec_algorithm = NonForwardingExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=exec_algorithm_id),
    )

    assert exec_algorithm.exec_algorithm_id == exec_algorithm_id


def test_execution_algorithm_preserves_explicit_id_created_inside_subclass():
    exec_algorithm = InternalConfigExecutionAlgorithm()

    assert exec_algorithm.exec_algorithm_id == ExecAlgorithmId("INTERNAL-CONFIG")


def test_execution_algorithm_uses_actor_id_created_inside_subclass():
    exec_algorithm = InternalActorIdExecutionAlgorithm()

    assert exec_algorithm.exec_algorithm_id == ExecAlgorithmId("INTERNAL-ACTOR-ID")


def test_execution_algorithm_rejects_non_ascii_derived_id():
    non_ascii_algorithm = type("Strategy\u00e9", (ExecutionAlgorithm,), {})

    with pytest.raises(ValueError, match="non-ASCII char"):
        non_ascii_algorithm()


def test_add_exec_algorithms_registers_distinct_class_derived_ids():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    first = FirstDefaultExecutionAlgorithm()
    second = SecondDefaultExecutionAlgorithm()

    engine.add_exec_algorithms([first, second])

    assert first.exec_algorithm_id == ExecAlgorithmId("FirstDefaultExecutionAlgorithm")
    assert first.is_registered() is True
    assert second.exec_algorithm_id == ExecAlgorithmId("SecondDefaultExecutionAlgorithm")
    assert second.is_registered() is True
    engine.dispose()


def test_execution_algorithm_deny_order_updates_cache_once():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=ExecAlgorithmId("PY-DENY")),
    )
    engine.add_exec_algorithm(exec_algorithm)
    order = create_market_order()
    reason = "VALIDATION_FAILED: invalid Python execution schedule"

    exec_algorithm.deny_order(order, reason)
    exec_algorithm.deny_order(order, reason)

    cached_order = exec_algorithm.cache.order(order.client_order_id)
    assert cached_order.status == OrderStatus.DENIED
    assert cached_order.event_count == 2
    assert isinstance(cached_order.last_event, OrderDenied)
    assert cached_order.last_event.reason == reason
    assert cached_order.last_event.strategy_id == order.strategy_id
    engine.dispose()


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
        config={"exec_algorithm_id": "BACKTEST-ALGO-RUNNING"},
    )

    try:
        engine.run(streaming=True)
        with pytest.raises(RuntimeError, match="Cannot add execution algorithms to running trader"):
            engine.add_exec_algorithm_from_config(config)
        # Guard runs before constructing the user class, so the constructor never fires
        assert RequiredConfigBacktestExecAlgorithm.received_exec_algorithm_id is None
    finally:
        engine.dispose()


def test_add_exec_algorithm_from_config_registers_non_forwarding_subclass_under_config_id():
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
        config={"exec_algorithm_id": "BACKTEST-ALGO-NOFORWARD"},
    )
    engine.add_exec_algorithm_from_config(config)

    # Subclass omits forwarding config to super().__init__(); __new__ still retains it, so
    # the dup add collides on the configured id rather than a default.
    with pytest.raises(RuntimeError, match="'BACKTEST-ALGO-NOFORWARD' is already registered"):
        engine.add_exec_algorithm_from_config(config)

    engine.dispose()


def test_add_exec_algorithm_registers_constructed_v2_instance():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    exec_algorithm_id = ExecAlgorithmId("BACKTEST-V2-INSTANCE")
    exec_algorithm = ExecutionAlgorithm(
        ExecutionAlgorithmConfig(exec_algorithm_id=exec_algorithm_id),
    )

    engine.add_exec_algorithm(exec_algorithm)

    assert exec_algorithm.exec_algorithm_id == exec_algorithm_id
    assert exec_algorithm.is_registered() is True
    assert exec_algorithm.is_ready() is True
    assert exec_algorithm.portfolio.is_initialized() is False
    engine.dispose()


def test_add_exec_algorithm_registers_non_forwarding_instance_under_config_id():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_exec_algorithm(
        RequiredConfigBacktestExecAlgorithm(
            RequiredConfigBacktestExecAlgorithmConfig(exec_algorithm_id="BACKTEST-ALGO-INSTANCE"),
        ),
    )

    # Subclass omits forwarding config to super().__init__(); __new__ retains it, so the
    # instance registers under the configured id.
    with pytest.raises(RuntimeError, match="'BACKTEST-ALGO-INSTANCE' is already registered"):
        engine.add_exec_algorithm(
            RequiredConfigBacktestExecAlgorithm(
                RequiredConfigBacktestExecAlgorithmConfig(
                    exec_algorithm_id="BACKTEST-ALGO-INSTANCE",
                ),
            ),
        )

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


def create_market_order():
    return MarketOrder(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("DENY-001"),
        instrument_id=InstrumentId.from_str("AUD/USD.SIM"),
        client_order_id=ClientOrderId("O-PY-DENY"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        init_id=UUID4(),
        ts_init=0,
        time_in_force=TimeInForce.GTC,
        reduce_only=False,
        quote_quantity=False,
    )
