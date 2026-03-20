# ---------------------------------------------------------------------------
# Tests for Issue #3630: Support multiple identifiers in subscribe messages
# ---------------------------------------------------------------------------
# File: tests/unit_tests/common/test_batch_subscribe.py
#
# These tests validate the batch subscribe functionality added to Actor
# and the create_batch factory methods on message classes.
#
# To run:
#   pytest tests/unit_tests/common/test_batch_subscribe.py -v
# ---------------------------------------------------------------------------

import pytest

from nautilus_trader.data.messages import SubscribeBars
from nautilus_trader.data.messages import SubscribeQuoteTicks
from nautilus_trader.data.messages import SubscribeTradeTicks
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.data import TestDataStubs


# ============================================================================
# Helper fixtures
# ============================================================================

AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")


def _ts_init() -> int:
    """Return a fixed nanosecond timestamp for testing."""
    return 1_000_000_000


# ============================================================================
# Tests: SubscribeBars.create_batch
# ============================================================================


class TestSubscribeBarsCreateBatch:
    """Tests for SubscribeBars.create_batch static factory method."""

    def test_create_batch_returns_list_of_correct_length(self):
        # Arrange
        bar_types = [
            TestDataStubs.bartype_adabtc_binance_1min_last(),
            TestDataStubs.bartype_audusd_1min_bid(),
        ]

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=None,
            venue=Venue("BINANCE"),
            ts_init=_ts_init(),
        )

        # Assert
        assert len(commands) == 2
        assert all(isinstance(c, SubscribeBars) for c in commands)

    def test_create_batch_each_command_has_correct_bar_type(self):
        # Arrange
        bar_types = [
            TestDataStubs.bartype_adabtc_binance_1min_last(),
            TestDataStubs.bartype_audusd_1min_bid(),
        ]

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=None,
            venue=Venue("BINANCE"),
            ts_init=_ts_init(),
        )

        # Assert
        assert commands[0].bar_type == bar_types[0]
        assert commands[1].bar_type == bar_types[1]

    def test_create_batch_each_command_has_unique_id(self):
        # Arrange
        bar_types = [
            TestDataStubs.bartype_adabtc_binance_1min_last(),
            TestDataStubs.bartype_audusd_1min_bid(),
        ]

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=None,
            venue=Venue("BINANCE"),
            ts_init=_ts_init(),
        )

        # Assert
        assert commands[0].id != commands[1].id

    def test_create_batch_propagates_params_as_copies(self):
        # Arrange
        bar_types = [
            TestDataStubs.bartype_adabtc_binance_1min_last(),
            TestDataStubs.bartype_audusd_1min_bid(),
        ]
        params = {"update_catalog": True}

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=None,
            venue=Venue("BINANCE"),
            ts_init=_ts_init(),
            params=params,
        )

        # Assert — each command gets its own copy of params
        assert commands[0].params == {"update_catalog": True}
        assert commands[1].params == {"update_catalog": True}
        # Mutating one should not affect the other
        commands[0].params["extra"] = "x"
        assert "extra" not in commands[1].params

    def test_create_batch_with_single_item(self):
        # Arrange
        bar_types = [TestDataStubs.bartype_audusd_1min_bid()]

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        # Assert
        assert len(commands) == 1
        assert commands[0].bar_type == bar_types[0]

    def test_create_batch_raises_on_empty_list(self):
        # Act & Assert
        with pytest.raises(ValueError):
            SubscribeBars.create_batch(
                bar_types=[],
                client_id=None,
                venue=Venue("SIM"),
                ts_init=_ts_init(),
            )

    def test_create_batch_with_client_id(self):
        # Arrange
        bar_types = [TestDataStubs.bartype_audusd_1min_bid()]
        client_id = ClientId("CUSTOM")

        # Act
        commands = SubscribeBars.create_batch(
            bar_types=bar_types,
            client_id=client_id,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        # Assert
        assert commands[0].client_id == client_id


# ============================================================================
# Tests: SubscribeQuoteTicks.create_batch
# ============================================================================


class TestSubscribeQuoteTicksCreateBatch:
    """Tests for SubscribeQuoteTicks.create_batch static factory method."""

    def test_create_batch_returns_correct_types(self):
        # Arrange
        ids = [
            AUDUSD_SIM.id,
            GBPUSD_SIM.id,
            USDJPY_SIM.id,
        ]

        # Act
        commands = SubscribeQuoteTicks.create_batch(
            instrument_ids=ids,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        # Assert
        assert len(commands) == 3
        assert all(isinstance(c, SubscribeQuoteTicks) for c in commands)

    def test_create_batch_each_has_correct_instrument_id(self):
        # Arrange
        ids = [AUDUSD_SIM.id, GBPUSD_SIM.id]

        # Act
        commands = SubscribeQuoteTicks.create_batch(
            instrument_ids=ids,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        # Assert
        assert commands[0].instrument_id == ids[0]
        assert commands[1].instrument_id == ids[1]

    def test_create_batch_raises_on_empty_list(self):
        with pytest.raises(ValueError):
            SubscribeQuoteTicks.create_batch(
                instrument_ids=[],
                client_id=None,
                venue=Venue("SIM"),
                ts_init=_ts_init(),
            )

    def test_create_batch_unique_command_ids(self):
        ids = [AUDUSD_SIM.id, GBPUSD_SIM.id]

        commands = SubscribeQuoteTicks.create_batch(
            instrument_ids=ids,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        assert commands[0].id != commands[1].id


# ============================================================================
# Tests: SubscribeTradeTicks.create_batch
# ============================================================================


class TestSubscribeTradeTicksCreateBatch:
    """Tests for SubscribeTradeTicks.create_batch static factory method."""

    def test_create_batch_returns_correct_types(self):
        ids = [AUDUSD_SIM.id, GBPUSD_SIM.id]

        commands = SubscribeTradeTicks.create_batch(
            instrument_ids=ids,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        assert len(commands) == 2
        assert all(isinstance(c, SubscribeTradeTicks) for c in commands)

    def test_create_batch_each_has_correct_instrument_id(self):
        ids = [AUDUSD_SIM.id, GBPUSD_SIM.id]

        commands = SubscribeTradeTicks.create_batch(
            instrument_ids=ids,
            client_id=None,
            venue=Venue("SIM"),
            ts_init=_ts_init(),
        )

        assert commands[0].instrument_id == ids[0]
        assert commands[1].instrument_id == ids[1]

    def test_create_batch_raises_on_empty_list(self):
        with pytest.raises(ValueError):
            SubscribeTradeTicks.create_batch(
                instrument_ids=[],
                client_id=None,
                venue=Venue("SIM"),
                ts_init=_ts_init(),
            )
