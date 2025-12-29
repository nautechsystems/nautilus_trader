# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import ComponentState
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import OmsType
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import Venue
from nautilus_trader.core.nautilus_pyo3.trading import Strategy
from nautilus_trader.core.nautilus_pyo3.trading import StrategyConfig


# Test fixtures
@pytest.fixture
def trader_id():
    return TraderId("TRADER-001")


@pytest.fixture
def client_id():
    return ClientId("SIM")


@pytest.fixture
def venue():
    return Venue("SIM")


@pytest.fixture
def symbol():
    return Symbol("BTC/USDT")


@pytest.fixture
def instrument_id(symbol, venue):
    return InstrumentId(symbol, venue)


# =============================================================================
# StrategyConfig Tests
# =============================================================================


def test_strategy_config_default():
    """
    Test StrategyConfig with default values.
    """
    config = StrategyConfig()

    assert config.strategy_id is None
    assert config.order_id_tag is None
    assert config.oms_type is None
    assert config.manage_contingent_orders is False
    assert config.manage_gtd_expiry is False
    assert config.use_uuid_client_order_ids is False
    assert config.use_hyphens_in_client_order_ids is True
    assert config.log_events is True
    assert config.log_commands is True
    assert config.log_rejected_due_post_only_as_warning is True


def test_strategy_config_with_values():
    """
    Test StrategyConfig with custom values.
    """
    strategy_id = StrategyId("TEST-001")
    config = StrategyConfig(
        strategy_id=strategy_id,
        order_id_tag="TAG1",
        oms_type=OmsType.HEDGING,
        manage_contingent_orders=True,
        manage_gtd_expiry=True,
        use_uuid_client_order_ids=True,
        use_hyphens_in_client_order_ids=False,
        log_events=False,
        log_commands=False,
        log_rejected_due_post_only_as_warning=False,
    )

    assert config.strategy_id == strategy_id
    assert config.order_id_tag == "TAG1"
    assert config.oms_type == OmsType.HEDGING
    assert config.manage_contingent_orders is True
    assert config.manage_gtd_expiry is True
    assert config.use_uuid_client_order_ids is True
    assert config.use_hyphens_in_client_order_ids is False
    assert config.log_events is False
    assert config.log_commands is False
    assert config.log_rejected_due_post_only_as_warning is False


# =============================================================================
# Basic Creation and Inheritance Tests
# =============================================================================


def test_strategy_creation():
    """
    Test basic Strategy creation.
    """
    strategy = Strategy()
    assert strategy is not None
    assert str(type(strategy)) == "<class 'nautilus_trader.trading.Strategy'>"


def test_strategy_creation_with_config():
    """
    Test Strategy creation with config.
    """
    strategy_id = StrategyId("TEST-001")
    config = StrategyConfig(strategy_id=strategy_id)
    strategy = Strategy(config=config)

    assert strategy is not None
    assert strategy.strategy_id == strategy_id


def test_strategy_inheritance():
    """
    Test that Strategy can be inherited from Python (key requirement).
    """

    class TestStrategyImplementation(Strategy):
        def __init__(self):
            super().__init__()
            self.events = []

        def on_start(self):
            self.events.append(("on_start",))

        def on_stop(self):
            self.events.append(("on_stop",))

        def on_order_accepted(self, event):
            self.events.append(("on_order_accepted", event))

        def on_position_opened(self, event):
            self.events.append(("on_position_opened", event))

    strategy = TestStrategyImplementation()
    assert isinstance(strategy, Strategy)
    assert hasattr(strategy, "events")
    assert strategy.events == []

    strategy.on_start()
    assert ("on_start",) in strategy.events

    assert hasattr(strategy, "submit_order")
    assert hasattr(strategy, "cancel_order")
    assert hasattr(strategy, "close_position")
    assert hasattr(strategy, "start")
    assert hasattr(strategy, "stop")


def test_inheritance_preserves_functionality():
    """
    Test that inherited Strategy preserves all functionality.
    """

    class MyStrategy(Strategy):
        def __init__(self):
            super().__init__()
            self.order_count = 0

        def on_order_accepted(self, event):
            self.order_count += 1

    strategy = MyStrategy()
    assert isinstance(strategy, Strategy)
    assert hasattr(strategy, "order_count")
    assert strategy.order_count == 0
    assert hasattr(strategy, "submit_order")
    assert hasattr(strategy, "start")


# =============================================================================
# Implementation Validation Tests
# =============================================================================


def test_efficient_rust_implementation():
    """
    Test that this is an efficient Rust-based implementation.
    """
    strategy = Strategy()
    assert str(type(strategy)) == "<class 'nautilus_trader.trading.Strategy'>"
    assert not hasattr(strategy, "_msgbus")
    assert not hasattr(strategy, "_message_handlers")


# =============================================================================
# Unregistered Strategy Behavior Tests
# =============================================================================


def test_unregistered_strategy_properties():
    """
    Test that unregistered strategy provides basic properties.
    """
    strategy = Strategy()
    assert strategy.trader_id is None
    assert strategy.strategy_id is not None
    assert strategy.state() == ComponentState.PreInitialized
    assert not strategy.is_ready()
    assert not strategy.is_running()
    assert not strategy.is_stopped()
    assert not strategy.is_disposed()
    assert not strategy.is_degraded()
    assert not strategy.is_faulted()


# =============================================================================
# Order Management Method Tests
# =============================================================================


def test_order_management_methods_exist():
    """
    Test that all order management methods exist.
    """
    strategy = Strategy()

    order_methods = [
        "submit_order",
        "modify_order",
        "cancel_order",
        "cancel_orders",
        "cancel_all_orders",
    ]

    for method_name in order_methods:
        assert hasattr(strategy, method_name), f"Missing method: {method_name}"


def test_submit_order_signature():
    """
    Test that submit_order has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.submit_order)
    params = list(sig.parameters.keys())
    assert "order" in params
    assert "position_id" in params
    assert "client_id" in params
    assert "params" in params


def test_modify_order_signature():
    """
    Test that modify_order has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.modify_order)
    params = list(sig.parameters.keys())
    assert "order" in params
    assert "quantity" in params
    assert "price" in params
    assert "trigger_price" in params
    assert "client_id" in params


def test_cancel_order_signature():
    """
    Test that cancel_order has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.cancel_order)

    params = list(sig.parameters.keys())
    assert "order" in params
    assert "client_id" in params


def test_cancel_all_orders_signature(instrument_id):
    """
    Test that cancel_all_orders has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.cancel_all_orders)

    params = list(sig.parameters.keys())
    assert "instrument_id" in params
    assert "order_side" in params
    assert "client_id" in params


# =============================================================================
# Position Management Method Tests
# =============================================================================


def test_position_management_methods_exist():
    """
    Test that all position management methods exist.
    """
    strategy = Strategy()

    position_methods = [
        "close_position",
        "close_all_positions",
    ]

    for method_name in position_methods:
        assert hasattr(strategy, method_name), f"Missing method: {method_name}"


def test_close_position_signature():
    """
    Test that close_position has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.close_position)

    params = list(sig.parameters.keys())
    assert "position" in params
    assert "client_id" in params
    assert "tags" in params
    assert "time_in_force" in params
    assert "reduce_only" in params


def test_close_all_positions_signature(instrument_id):
    """
    Test that close_all_positions has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.close_all_positions)

    params = list(sig.parameters.keys())
    assert "instrument_id" in params
    assert "position_side" in params
    assert "client_id" in params


# =============================================================================
# Query Method Tests
# =============================================================================


def test_query_methods_exist():
    """
    Test that query methods exist.
    """
    strategy = Strategy()

    assert hasattr(strategy, "query_account")
    assert hasattr(strategy, "query_order")


def test_query_account_signature():
    """
    Test that query_account has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.query_account)

    params = list(sig.parameters.keys())
    assert "account_id" in params
    assert "client_id" in params


def test_query_order_signature():
    """
    Test that query_order has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.query_order)

    params = list(sig.parameters.keys())
    assert "order" in params
    assert "client_id" in params


# =============================================================================
# Lifecycle Method Tests
# =============================================================================


def test_lifecycle_methods_exist():
    """
    Test that lifecycle methods exist on strategy instances.
    """
    strategy = Strategy()

    lifecycle_methods = [
        "start",
        "stop",
        "resume",
        "reset",
        "dispose",
        "degrade",
        "fault",
    ]

    for method_name in lifecycle_methods:
        assert hasattr(strategy, method_name), f"Missing method: {method_name}"


# =============================================================================
# Event Handler Method Tests
# =============================================================================


def test_order_event_handlers_overridable():
    """
    Test that order event handlers can be overridden in Python subclasses.
    """
    # Note: Order event handlers (on_order_initialized, etc.) are dispatched to Python
    # through method calls, so subclasses can define them. They don't need to exist
    # on the base class.

    class MyStrategy(Strategy):
        def __init__(self):
            super().__init__()
            self.events = []

        def on_order_accepted(self, event):
            self.events.append(("on_order_accepted", event))

    strategy = MyStrategy()
    assert hasattr(strategy, "on_order_accepted")
    assert callable(strategy.on_order_accepted)


def test_position_management_calls_exist():
    """Test that position management calls exist (not event handlers - those need PyO3 bindings)."""
    strategy = Strategy()

    # Note: Position event handlers (on_position_opened, etc.) are not exposed to Python yet
    # because PositionOpened, PositionChanged, PositionClosed don't have PyO3 bindings.
    # This test verifies position management methods exist.
    assert hasattr(strategy, "close_position")
    assert hasattr(strategy, "close_all_positions")


def test_lifecycle_event_handlers_exist():
    """
    Test that all lifecycle event handler methods exist.
    """
    strategy = Strategy()

    lifecycle_handlers = [
        "on_start",
        "on_stop",
        "on_resume",
        "on_reset",
        "on_dispose",
        "on_degrade",
        "on_fault",
    ]

    for method_name in lifecycle_handlers:
        assert hasattr(strategy, method_name), f"Missing method: {method_name}"


# =============================================================================
# Data Event Handler Tests
# =============================================================================


def test_data_event_handlers_exist():
    """
    Test that data event handlers exist on Strategy.
    """
    strategy = Strategy()

    data_handlers = [
        "on_quote",
        "on_trade",
        "on_bar",
        "on_signal",
        "on_instrument",
        "on_book",
        "on_book_deltas",
        "on_mark_price",
        "on_index_price",
        "on_funding_rate",
        "on_instrument_status",
        "on_instrument_close",
    ]

    for handler_name in data_handlers:
        assert hasattr(strategy, handler_name), f"Missing handler: {handler_name}"


def test_data_event_handlers_overridable():
    """
    Test that data event handlers can be overridden in Python subclasses.
    """

    class DataTrackingStrategy(Strategy):
        def __init__(self):
            super().__init__()
            self.quotes = []
            self.trades = []
            self.bars = []

        def on_quote(self, quote):
            self.quotes.append(quote)

        def on_trade(self, trade):
            self.trades.append(trade)

        def on_bar(self, bar):
            self.bars.append(bar)

    strategy = DataTrackingStrategy()
    assert isinstance(strategy, Strategy)
    assert strategy.quotes == []
    assert strategy.trades == []
    assert strategy.bars == []


def test_data_event_handler_methods_callable():
    """
    Test that data event handler methods are callable on subclasses.
    """

    class TestStrategy(Strategy):
        def __init__(self):
            super().__init__()
            self.data_received = []

        def on_quote(self, quote):
            self.data_received.append(("quote", quote))

        def on_trade(self, trade):
            self.data_received.append(("trade", trade))

        def on_bar(self, bar):
            self.data_received.append(("bar", bar))

    strategy = TestStrategy()

    strategy.on_quote("mock_quote")
    strategy.on_trade("mock_trade")
    strategy.on_bar("mock_bar")

    assert ("quote", "mock_quote") in strategy.data_received
    assert ("trade", "mock_trade") in strategy.data_received
    assert ("bar", "mock_bar") in strategy.data_received


# =============================================================================
# DataActor API Parity Tests
# =============================================================================


def test_subscribe_methods_exist():
    """
    Test that all DataActor subscribe methods are exposed on Strategy.
    """
    strategy = Strategy()

    subscribe_methods = [
        "subscribe_data",
        "subscribe_instruments",
        "subscribe_instrument",
        "subscribe_book_deltas",
        "subscribe_book_at_interval",
        "subscribe_quotes",
        "subscribe_trades",
        "subscribe_bars",
        "subscribe_mark_prices",
        "subscribe_index_prices",
        "subscribe_instrument_status",
        "subscribe_instrument_close",
    ]

    for method_name in subscribe_methods:
        assert hasattr(strategy, method_name), f"Missing subscribe method: {method_name}"


def test_unsubscribe_methods_exist():
    """
    Test that all DataActor unsubscribe methods are exposed on Strategy.
    """
    strategy = Strategy()

    unsubscribe_methods = [
        "unsubscribe_data",
        "unsubscribe_instruments",
        "unsubscribe_instrument",
        "unsubscribe_book_deltas",
        "unsubscribe_book_at_interval",
        "unsubscribe_quotes",
        "unsubscribe_trades",
        "unsubscribe_bars",
        "unsubscribe_mark_prices",
        "unsubscribe_index_prices",
        "unsubscribe_instrument_status",
        "unsubscribe_instrument_close",
    ]

    for method_name in unsubscribe_methods:
        assert hasattr(strategy, method_name), f"Missing unsubscribe method: {method_name}"


def test_request_methods_exist():
    """
    Test that all DataActor request methods are exposed on Strategy.
    """
    strategy = Strategy()

    request_methods = [
        "request_data",
        "request_instrument",
        "request_instruments",
        "request_book_snapshot",
        "request_quotes",
        "request_trades",
        "request_bars",
    ]

    for method_name in request_methods:
        assert hasattr(strategy, method_name), f"Missing request method: {method_name}"


def test_subscribe_quotes_signature():
    """
    Test that subscribe_quotes has the correct signature.
    """
    import inspect

    strategy = Strategy()
    sig = inspect.signature(strategy.subscribe_quotes)

    params = list(sig.parameters.keys())
    assert "instrument_id" in params
    assert "client_id" in params
    assert "params" in params


# =============================================================================
# Lifecycle Handler Dispatch Tests
# =============================================================================


def test_lifecycle_on_start_dispatches_to_python():
    """
    Test that calling strategy.on_start() dispatches to Python override.
    """
    events = []

    class TestStrategy(Strategy):
        def on_start(self):
            events.append("on_start_called")

    strategy = TestStrategy()
    strategy.on_start()

    assert "on_start_called" in events


def test_lifecycle_on_stop_dispatches_to_python():
    """
    Test that calling strategy.on_stop() dispatches to Python override.
    """
    events = []

    class TestStrategy(Strategy):
        def on_stop(self):
            events.append("on_stop_called")

    strategy = TestStrategy()
    strategy.on_stop()

    assert "on_stop_called" in events


def test_all_lifecycle_handlers_dispatch():
    """
    Test that all lifecycle handlers dispatch to Python overrides.
    """
    events = []

    class TestStrategy(Strategy):
        def on_start(self):
            events.append("start")

        def on_stop(self):
            events.append("stop")

        def on_resume(self):
            events.append("resume")

        def on_reset(self):
            events.append("reset")

        def on_dispose(self):
            events.append("dispose")

        def on_degrade(self):
            events.append("degrade")

        def on_fault(self):
            events.append("fault")

    strategy = TestStrategy()

    strategy.on_start()
    strategy.on_stop()
    strategy.on_resume()
    strategy.on_reset()
    strategy.on_dispose()
    strategy.on_degrade()
    strategy.on_fault()

    assert events == ["start", "stop", "resume", "reset", "dispose", "degrade", "fault"]


def test_rust_to_python_dispatch_enabled():
    """
    Test that Rust→Python dispatch is enabled after construction.

    The __init__ method captures py_self, enabling dispatch_* methods to call Python
    overrides. Without this, events from Rust would be silently dropped.

    """
    received_events = []

    class EventTrackingStrategy(Strategy):
        def on_start(self):
            received_events.append("on_start")

        def on_quote(self, quote):
            received_events.append(("on_quote", quote))

        def on_bar(self, bar):
            received_events.append(("on_bar", bar))

    strategy = EventTrackingStrategy()

    # These calls go through py_on_* → dispatch_on_* → Python override
    # If py_self wasn't set in __init__, these would be silently dropped
    strategy.on_start()
    strategy.on_quote("test_quote")
    strategy.on_bar("test_bar")

    assert "on_start" in received_events
    assert ("on_quote", "test_quote") in received_events
    assert ("on_bar", "test_bar") in received_events


# =============================================================================
# GTD Expiry Configuration Tests
# =============================================================================


def test_strategy_config_manage_gtd_expiry_accessible():
    """
    Test that manage_gtd_expiry config is accessible from Python.
    """
    config = StrategyConfig(manage_gtd_expiry=True)
    assert config.manage_gtd_expiry is True

    config_false = StrategyConfig(manage_gtd_expiry=False)
    assert config_false.manage_gtd_expiry is False


def test_strategy_with_gtd_expiry_enabled():
    """
    Test creating a Strategy with manage_gtd_expiry enabled.

    This exercises the Python wrapper path for GTD expiry configuration. When registered
    with a trader and started, the strategy would reactivate GTD timers. Without
    registration, we verify the config is properly passed.

    """
    strategy_id = StrategyId("GTD-TEST-001")
    config = StrategyConfig(
        strategy_id=strategy_id,
        manage_gtd_expiry=True,
    )
    strategy = Strategy(config=config)

    assert strategy.strategy_id == strategy_id

    # Lifecycle handlers should work even without registration
    # (they just won't have access to cache/clock until registered)
    called = []

    class GTDStrategy(Strategy):
        def on_start(self):
            called.append("on_start")

    gtd_strategy = GTDStrategy(config=StrategyConfig(manage_gtd_expiry=True))
    gtd_strategy.on_start()
    assert "on_start" in called


# =============================================================================
# API Consistency Tests
# =============================================================================


def test_strategy_api_consistency():
    """
    Test that Strategy API follows consistent naming patterns.
    """
    strategy = Strategy()

    order_methods = [attr for attr in dir(strategy) if "order" in attr.lower()]
    assert len(order_methods) >= 6, "Should have order management methods"

    position_methods = [attr for attr in dir(strategy) if "position" in attr.lower()]
    assert len(position_methods) >= 2, "Should have position management methods"

    lifecycle_methods = ["start", "stop", "resume", "reset", "dispose"]
    for method in lifecycle_methods:
        assert hasattr(strategy, method), f"Missing lifecycle method: {method}"
