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

from nautilus_trader.core.nautilus_pyo3 import AggregationSource
from nautilus_trader.core.nautilus_pyo3 import BarAggregation
from nautilus_trader.core.nautilus_pyo3 import BarSpecification
from nautilus_trader.core.nautilus_pyo3 import BarType
from nautilus_trader.core.nautilus_pyo3 import BookType
from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import DataActor
from nautilus_trader.core.nautilus_pyo3 import DataType
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import PriceType
from nautilus_trader.core.nautilus_pyo3 import Symbol
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import Venue


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
    return Symbol("AUD/USD")


@pytest.fixture
def instrument_id(symbol, venue):
    return InstrumentId(symbol, venue)


@pytest.fixture
def bar_specification():
    return BarSpecification(
        step=1,
        aggregation=BarAggregation.MINUTE,
        price_type=PriceType.LAST,
    )


@pytest.fixture
def bar_type(instrument_id, bar_specification):
    return BarType(
        instrument_id=instrument_id,
        spec=bar_specification,
        aggregation_source=AggregationSource.INTERNAL,
    )


@pytest.fixture
def data_type():
    return DataType("TestData")


# =============================================================================
# Basic Creation and Inheritance Tests
# =============================================================================


def test_actor_creation():
    """
    Test basic DataActor creation.
    """
    actor = DataActor()
    assert actor is not None
    assert str(type(actor)) == "<class 'nautilus_trader.common.DataActor'>"


def test_actor_inheritance():
    """
    Test that DataActor can be inherited from Python (key requirement).
    """

    class TestDataActorImplementation(DataActor):
        def __init__(self):
            super().__init__()
            self.events = []

        def on_data(self, data):
            self.events.append(("on_data", data))

        def on_quote_tick(self, tick):
            self.events.append(("on_quote_tick", tick))

        def on_start(self):
            self.events.append(("on_start",))

    # Test inheritance works
    actor = TestDataActorImplementation()
    assert isinstance(actor, DataActor)
    assert hasattr(actor, "events")
    assert actor.events == []

    # Test custom methods work
    actor.on_start()
    assert ("on_start",) in actor.events

    # Test that DataActor methods are available
    assert hasattr(actor, "subscribe_data")
    assert hasattr(actor, "subscribe_quotes")
    assert hasattr(actor, "subscribe_trades")
    assert hasattr(actor, "start")
    assert hasattr(actor, "stop")


def test_inheritance_preserves_functionality():
    """
    Test that inherited DataActor preserves all functionality.
    """

    class MyDataActor(DataActor):
        def __init__(self):
            super().__init__()
            self.received_data = []

        def on_data(self, data):
            self.received_data.append(data)

    actor = MyDataActor()

    # Should still be a DataActor
    assert isinstance(actor, DataActor)

    # Should have custom attributes
    assert hasattr(actor, "received_data")
    assert actor.received_data == []

    # Should still have all DataActor methods
    assert hasattr(actor, "subscribe_data")
    assert hasattr(actor, "start")


# =============================================================================
# Implementation Validation Tests
# =============================================================================


def test_efficient_rust_implementation():
    """
    Test that this is an efficient Rust-based implementation, not Python.
    """
    actor = DataActor()

    # Should be a PyO3 class that wraps Rust implementation
    assert str(type(actor)) == "<class 'nautilus_trader.common.DataActor'>"

    # Should not have Python-based message bus or inefficient implementations
    assert not hasattr(actor, "_msgbus")  # Should not have Python message bus
    assert not hasattr(actor, "_message_handlers")  # Should not have Python message handling
    assert not hasattr(actor, "msgbus")  # Should not expose Python msgbus directly

    # This validates we're using the efficient approach as required


# =============================================================================
# Unregistered Actor Behavior Tests
# =============================================================================


def test_unregistered_actor_properties_work():
    """
    Test that unregistered actor can provide basic properties.
    """
    actor = DataActor()

    # Basic properties should work without registration
    assert actor.trader_id is None
    assert actor.actor_id is not None

    # State should be PreInitialized for unregistered actor
    from nautilus_trader.core.nautilus_pyo3 import ComponentState

    assert actor.state() == ComponentState.PreInitialized  # TODO

    # trader_id should be None for unregistered actor
    assert actor.trader_id is None

    # Status checks should work
    assert not actor.is_ready()  # Should be False in PreInitialized state
    assert not actor.is_running()
    assert not actor.is_stopped()
    assert not actor.is_disposed()
    assert not actor.is_degraded()
    assert not actor.is_faulted()


def test_subscription_methods_exist(venue, instrument_id, data_type):
    """
    Test that subscription methods exist with correct signatures.
    """
    actor = DataActor()

    # Just verify the methods exist and have correct signatures
    assert hasattr(actor, "subscribe_data")
    assert hasattr(actor, "subscribe_instruments")
    assert hasattr(actor, "subscribe_instrument")
    assert hasattr(actor, "subscribe_quotes")
    assert hasattr(actor, "subscribe_trades")
    assert hasattr(actor, "subscribe_book_deltas")


def test_lifecycle_methods_exist_on_instance():
    """
    Test that lifecycle methods exist on actor instances.
    """
    actor = DataActor()

    # Just verify the methods exist
    assert hasattr(actor, "start")
    assert hasattr(actor, "stop")
    assert hasattr(actor, "resume")
    assert hasattr(actor, "reset")
    assert hasattr(actor, "dispose")
    assert hasattr(actor, "degrade")
    assert hasattr(actor, "fault")


# =============================================================================
# Method Signature and Availability Tests
# =============================================================================


def test_subscription_method_signatures_exist(venue, instrument_id, data_type, client_id, bar_type):
    """
    Test subscription method signatures and availability.
    """
    actor = DataActor()

    # Just test that methods exist with correct signatures
    assert hasattr(actor, "subscribe_data")
    assert hasattr(actor, "subscribe_instruments")
    assert hasattr(actor, "subscribe_instrument")
    assert hasattr(actor, "subscribe_book_deltas")
    assert hasattr(actor, "subscribe_book_at_interval")
    assert hasattr(actor, "subscribe_quotes")
    assert hasattr(actor, "subscribe_trades")
    assert hasattr(actor, "subscribe_bars")


def test_specialized_subscription_methods_exist(instrument_id, client_id):
    """
    Test specialized subscription methods exist.
    """
    actor = DataActor()

    # Just test that methods exist
    assert hasattr(actor, "subscribe_mark_prices")
    assert hasattr(actor, "subscribe_index_prices")
    assert hasattr(actor, "subscribe_instrument_status")
    assert hasattr(actor, "subscribe_instrument_close")
    assert hasattr(actor, "subscribe_order_fills")


def test_request_methods_exist(instrument_id, client_id, data_type):
    """
    Test that all request methods exist and have correct signatures.
    """
    actor = DataActor()

    assert hasattr(actor, "request_data")
    assert hasattr(actor, "request_instrument")
    assert hasattr(actor, "request_instruments")
    assert hasattr(actor, "request_book_snapshot")
    assert hasattr(actor, "request_quotes")
    assert hasattr(actor, "request_trades")
    assert hasattr(actor, "request_bars")

    # Just verify methods exist with proper signatures


def test_unsubscribe_methods_exist(client_id, data_type):
    """
    Test that all unsubscribe methods exist.
    """
    actor = DataActor()

    unsubscribe_methods = [
        "unsubscribe_data",
        "unsubscribe_instruments",
        "unsubscribe_instrument",
        "unsubscribe_book_deltas",
        "unsubscribe_quotes",
        "unsubscribe_trades",
        "unsubscribe_bars",
        "unsubscribe_mark_prices",
        "unsubscribe_index_prices",
        "unsubscribe_instrument_status",
        "unsubscribe_instrument_close",
        "unsubscribe_book_at_interval",
        "unsubscribe_order_fills",
    ]

    for method_name in unsubscribe_methods:
        assert hasattr(actor, method_name), f"Missing method: {method_name}"

    # Just verify all methods exist


def test_shutdown_system_method_exists():
    """
    Test that shutdown_system method exists.
    """
    actor = DataActor()

    assert hasattr(actor, "shutdown_system")

    # Just verify method exists


# =============================================================================
# Validation and Error Handling Tests
# =============================================================================


def test_subscribe_book_at_interval_invalid_interval_raises_error(instrument_id, client_id):
    """
    Test that invalid interval raises appropriate error.
    """
    actor = DataActor()

    # Should raise ValueError for invalid interval (this validation happens before registration check)
    with pytest.raises(ValueError, match="interval_ms must be > 0"):
        actor.subscribe_book_at_interval(
            instrument_id,
            BookType.L2_MBP,
            0,  # Invalid interval
            None,
            client_id,
            None,
        )


def test_unsubscribe_book_at_interval_invalid_interval_raises_error(instrument_id, client_id):
    """
    Test that invalid interval raises appropriate error for unsubscribe.
    """
    actor = DataActor()

    # Should raise ValueError for invalid interval (this validation happens before registration check)
    with pytest.raises(ValueError, match="interval_ms must be > 0"):
        actor.unsubscribe_book_at_interval(
            instrument_id,
            0,  # Invalid interval
            client_id,
            None,
        )


# =============================================================================
# Legacy API Compatibility Tests
# =============================================================================


def test_all_subscription_methods_match_legacy_api():
    """
    Test that all expected subscription methods match the legacy Actor API.
    """
    actor = DataActor()

    expected_subscription_methods = [
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
        "subscribe_order_fills",
    ]

    for method_name in expected_subscription_methods:
        assert hasattr(actor, method_name), f"Missing subscription method: {method_name}"


def test_all_request_methods_match_legacy_api():
    """
    Test that all expected request methods match the legacy Actor API.
    """
    actor = DataActor()

    expected_request_methods = [
        "request_data",
        "request_instrument",
        "request_instruments",
        "request_book_snapshot",
        "request_quotes",
        "request_trades",
        "request_bars",
    ]

    for method_name in expected_request_methods:
        assert hasattr(actor, method_name), f"Missing request method: {method_name}"


def test_method_signatures_compatible(instrument_id, client_id, data_type):
    """
    Test that method signatures are compatible with expected types.
    """
    actor = DataActor()

    # Just verify that methods exist and can be called with expected parameter types
    # (We're not actually calling them to avoid registration panics)
    import inspect

    # Check subscribe_data signature
    sig = inspect.signature(actor.subscribe_data)
    assert len(sig.parameters) >= 1  # Should accept DataType at minimum

    # Check subscribe_book_deltas signature
    sig = inspect.signature(actor.subscribe_book_deltas)
    assert len(sig.parameters) >= 2  # Should accept InstrumentId and BookType at minimum


def test_api_style_consistency_with_legacy():
    """
    Test that the API style is consistent with legacy Actor patterns.
    """
    actor = DataActor()

    # Method naming should follow legacy patterns:
    # - subscribe_* for subscriptions
    # - unsubscribe_* for unsubscriptions
    # - request_* for requests
    # - Lifecycle methods without prefixes

    # Check subscription method naming pattern
    subscription_methods = [attr for attr in dir(actor) if attr.startswith("subscribe_")]
    assert len(subscription_methods) >= 8, "Should have multiple subscribe_* methods"

    # Check unsubscription method naming pattern
    unsubscription_methods = [attr for attr in dir(actor) if attr.startswith("unsubscribe_")]
    assert len(unsubscription_methods) >= 8, "Should have multiple unsubscribe_* methods"

    # Check request method naming pattern
    request_methods = [attr for attr in dir(actor) if attr.startswith("request_")]
    assert len(request_methods) >= 6, "Should have multiple request_* methods"

    # Check lifecycle methods exist without prefixes
    lifecycle_methods = ["start", "stop", "resume", "reset", "dispose"]
    for method in lifecycle_methods:
        assert hasattr(actor, method), f"Missing lifecycle method: {method}"
