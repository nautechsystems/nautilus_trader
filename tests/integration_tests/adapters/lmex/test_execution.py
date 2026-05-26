# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for ``LmexLiveExecutionClient`` order event handling and status mapping.

No live API calls or real NautilusTrader framework objects are constructed.
The execution client methods under test are called on a lightweight stub that
provides only the attributes each method actually touches.
"""

from __future__ import annotations

import types
from unittest.mock import MagicMock

import msgspec
import pytest


# ---------------------------------------------------------------------------
# Pure-function tests — _lmex_status_to_nautilus
# ---------------------------------------------------------------------------


class TestLmexStatusMapping:
    """Tests for the ``_lmex_status_to_nautilus`` status-code mapping."""

    def _map(self, code: int):
        from nautilus_trader.adapters.lmex.execution import _lmex_status_to_nautilus

        return _lmex_status_to_nautilus(code)

    def test_status_2_maps_to_accepted(self) -> None:
        """Status 2 (ORDER_INSERTED) → OrderStatus.ACCEPTED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(2) == OrderStatus.ACCEPTED

    def test_status_4_maps_to_filled(self) -> None:
        """Status 4 (ORDER_FULLY_TRANSACTED) → OrderStatus.FILLED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(4) == OrderStatus.FILLED

    def test_status_5_maps_to_partially_filled(self) -> None:
        """Status 5 (ORDER_PARTIALLY_TRANSACTED) → OrderStatus.PARTIALLY_FILLED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(5) == OrderStatus.PARTIALLY_FILLED

    def test_status_6_maps_to_canceled(self) -> None:
        """Status 6 (ORDER_CANCELLED) → OrderStatus.CANCELED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(6) == OrderStatus.CANCELED

    def test_status_7_maps_to_canceled(self) -> None:
        """Status 7 (STATUS_INACTIVE) → OrderStatus.CANCELED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(7) == OrderStatus.CANCELED

    def test_status_8_maps_to_accepted(self) -> None:
        """Status 8 (TRIGGER_INSERTED) → OrderStatus.ACCEPTED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(8) == OrderStatus.ACCEPTED

    def test_status_9_maps_to_triggered(self) -> None:
        """Status 9 (TRIGGER_ACTIVATED) → OrderStatus.TRIGGERED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(9) == OrderStatus.TRIGGERED

    def test_status_10_maps_to_rejected(self) -> None:
        """Status 10 (MARKET_UNAVAILABLE) → OrderStatus.REJECTED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(10) == OrderStatus.REJECTED

    def test_status_16_maps_to_rejected(self) -> None:
        """Status 16 (FAILED_ERROR) → OrderStatus.REJECTED."""
        from nautilus_trader.model.enums import OrderStatus

        assert self._map(16) == OrderStatus.REJECTED

    def test_unknown_status_raises_value_error(self) -> None:
        """An unrecognised status code raises ``ValueError``."""
        with pytest.raises(ValueError, match="Unknown LMEX order status code"):
            self._map(999)

    def test_all_defined_statuses_map_without_error(self) -> None:
        """Every ``LmexOrderStatus`` member maps without raising."""
        from nautilus_trader.adapters.lmex.enums import LmexOrderStatus

        for member in LmexOrderStatus:
            result = self._map(member.value)
            assert result is not None


# ---------------------------------------------------------------------------
# Stub factory for execution client
# ---------------------------------------------------------------------------


def _make_exec_stub() -> types.SimpleNamespace:
    """
    Build a minimal namespace for execution client method tests.

    Binds ``handle_ws_message`` and ``_handle_order_event`` from the real
    class, with all NT dependencies replaced by mocks.
    """
    from nautilus_trader.adapters.lmex.execution import LmexLiveExecutionClient
    from nautilus_trader.adapters.lmex.schemas.ws import LmexWsMsg, LmexWsOrderEventMsg
    from nautilus_trader.model.identifiers import StrategyId

    stub = types.SimpleNamespace()

    # Real decoders
    stub._dec_ws_envelope = msgspec.json.Decoder(LmexWsMsg)
    stub._dec_ws_order_events = msgspec.json.Decoder(LmexWsOrderEventMsg)

    stub._log = MagicMock()
    stub._clock = MagicMock()
    stub._clock.timestamp_ns.return_value = 1_779_786_400_000_000_000

    stub._cache = MagicMock()
    stub._cache.strategy_ids.return_value = [StrategyId("TestStrategy-001")]

    # Mock NT generate_* methods
    stub.generate_order_filled = MagicMock()
    stub.generate_order_canceled = MagicMock()
    stub.generate_order_rejected = MagicMock()
    stub.generate_order_submitted = MagicMock()
    stub.generate_order_accepted = MagicMock()
    stub.generate_order_cancel_rejected = MagicMock()
    stub.generate_order_modify_rejected = MagicMock()

    # Bind real methods
    stub.handle_ws_message = LmexLiveExecutionClient.handle_ws_message.__get__(
        stub, type(stub)
    )
    stub._handle_order_event = LmexLiveExecutionClient._handle_order_event.__get__(
        stub, type(stub)
    )

    return stub


def _make_mock_instrument(price_precision: int = 1, size_precision: int = 5):
    """Return a minimal instrument mock."""
    from nautilus_trader.model.currencies import Currency

    instrument = MagicMock()
    instrument.price_precision = price_precision
    instrument.size_precision = size_precision
    instrument.quote_currency = MagicMock(code="USD")
    return instrument


# ---------------------------------------------------------------------------
# handle_ws_message routing
# ---------------------------------------------------------------------------


class TestHandleWsMessage:
    """Tests for ``handle_ws_message`` → ``_handle_order_event`` routing."""

    def test_fill_event_calls_generate_order_filled(
        self, ws_order_fill: bytes
    ) -> None:
        """Status 4 (fill) triggers ``generate_order_filled``."""
        stub = _make_exec_stub()

        # Cache returns an order and instrument
        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.order_by_venue_order_id.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub.handle_ws_message(ws_order_fill)

        stub.generate_order_filled.assert_called_once()

    def test_cancel_event_calls_generate_order_canceled(
        self, ws_order_cancel: bytes
    ) -> None:
        """Status 6 (cancel) triggers ``generate_order_canceled``."""
        stub = _make_exec_stub()

        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.order_by_venue_order_id.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub.handle_ws_message(ws_order_cancel)

        stub.generate_order_canceled.assert_called_once()
        stub.generate_order_filled.assert_not_called()

    def test_invalid_json_logs_warning(self) -> None:
        """Malformed bytes log a warning and do not raise."""
        stub = _make_exec_stub()
        stub.handle_ws_message(b"not json !!!")

        stub._log.warning.assert_called_once()
        stub.generate_order_filled.assert_not_called()
        stub.generate_order_canceled.assert_not_called()

    def test_unknown_status_logs_warning(self) -> None:
        """An event with an unrecognised status code logs a warning."""
        stub = _make_exec_stub()

        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.order_by_venue_order_id.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        raw = (
            b'{"topic":"notificationsApi","data":[{'
            b'"symbol":"BTC-USD","orderId":1,"clOrderId":"x",'
            b'"status":999,"side":"BUY","size":0.01,"filledSize":0.0,'
            b'"price":76000.0,"avgFillPrice":0.0,"feeAmount":0.0,'
            b'"feeCurrency":"USD","tradeId":0,"timestamp":1779786400000}]}'
        )
        stub.handle_ws_message(raw)

        stub._log.warning.assert_called()
        stub.generate_order_filled.assert_not_called()
        stub.generate_order_canceled.assert_not_called()


# ---------------------------------------------------------------------------
# _handle_order_event field correctness
# ---------------------------------------------------------------------------


class TestHandleOrderEventFields:
    """Tests for correct field values passed to NT generate_* calls."""

    def test_fill_event_instrument_id(self, ws_order_fill: bytes) -> None:
        """Fill event instrument_id is derived from event symbol + LMEX venue."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        stub = _make_exec_stub()
        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub.handle_ws_message(ws_order_fill)

        call_kwargs = stub.generate_order_filled.call_args.kwargs
        assert call_kwargs["instrument_id"] == InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)

    def test_fill_event_venue_order_id(self, ws_order_fill: bytes) -> None:
        """Fill event venue_order_id matches fixture orderId 987654321."""
        from nautilus_trader.model.identifiers import VenueOrderId

        stub = _make_exec_stub()
        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub.handle_ws_message(ws_order_fill)

        call_kwargs = stub.generate_order_filled.call_args.kwargs
        assert call_kwargs["venue_order_id"] == VenueOrderId("987654321")

    def test_fill_event_last_price(self, ws_order_fill: bytes) -> None:
        """Fill last_px matches fixture avgFillPrice 76501.5."""
        stub = _make_exec_stub()
        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument(price_precision=1)

        stub.handle_ws_message(ws_order_fill)

        call_kwargs = stub.generate_order_filled.call_args.kwargs
        assert float(call_kwargs["last_px"]) == pytest.approx(76501.5, rel=1e-5)

    def test_cancel_event_client_order_id(self, ws_order_cancel: bytes) -> None:
        """Cancel event client_order_id matches fixture clOrderId."""
        from nautilus_trader.model.identifiers import ClientOrderId

        stub = _make_exec_stub()
        mock_order = MagicMock()
        mock_order.strategy_id = MagicMock()
        stub._cache.order.return_value = mock_order
        stub._cache.instrument.return_value = _make_mock_instrument()

        stub.handle_ws_message(ws_order_cancel)

        call_kwargs = stub.generate_order_canceled.call_args.kwargs
        # clOrderId from fixture is "my-order-001"
        assert call_kwargs["client_order_id"] == ClientOrderId("my-order-001")

    def test_no_strategy_logs_warning_and_skips(self) -> None:
        """When no strategy can be found the event is skipped with a warning."""
        stub = _make_exec_stub()
        stub._cache.order.return_value = None
        stub._cache.order_by_venue_order_id.return_value = None
        stub._cache.strategy_ids.return_value = []  # no strategies registered
        stub._cache.instrument.return_value = _make_mock_instrument()

        raw = (
            b'{"topic":"notificationsApi","data":[{'
            b'"symbol":"BTC-USD","orderId":1,"clOrderId":"x",'
            b'"status":4,"side":"BUY","size":0.01,"filledSize":0.01,'
            b'"price":76000.0,"avgFillPrice":76000.0,"feeAmount":0.0,'
            b'"feeCurrency":"USD","tradeId":1,"timestamp":1779786400000}]}'
        )
        stub.handle_ws_message(raw)

        stub._log.warning.assert_called()
        stub.generate_order_filled.assert_not_called()


# ---------------------------------------------------------------------------
# _modify_order rejects with explanation
# ---------------------------------------------------------------------------


class TestModifyOrderRejection:
    """Tests for cancel-replace fallback on order modification."""

    @pytest.mark.asyncio
    async def test_modify_order_generates_rejected(self) -> None:
        """``_modify_order`` always calls ``generate_order_modify_rejected``."""
        from nautilus_trader.adapters.lmex.execution import LmexLiveExecutionClient

        stub = _make_exec_stub()
        stub._modify_order = LmexLiveExecutionClient._modify_order.__get__(
            stub, type(stub)
        )

        command = MagicMock()
        command.strategy_id = MagicMock()
        command.instrument_id = MagicMock()
        command.client_order_id = MagicMock()
        command.venue_order_id = MagicMock()

        await stub._modify_order(command)

        stub.generate_order_modify_rejected.assert_called_once()
        call_kwargs = stub.generate_order_modify_rejected.call_args.kwargs
        assert "amendment" in call_kwargs["reason"].lower()
