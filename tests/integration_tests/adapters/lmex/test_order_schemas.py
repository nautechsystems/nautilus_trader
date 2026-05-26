# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for order management schema decoding.

All field names and shapes are verified against the live LMEX sandbox API
(``https://test-api.lmex.io/spot``) on 2026-05-26.  No live calls are made
here — JSON fixtures are loaded from ``tests/resources/http_responses/``.
"""

from __future__ import annotations

import msgspec
import pytest

from nautilus_trader.adapters.lmex.schemas.order import (
    LmexFill,
    LmexOpenOrder,
    LmexOrderResponse,
    LmexWalletEntry,
)


class TestLmexOrderResponseSchema:
    """Tests for ``LmexOrderResponse`` (submit / cancel endpoint)."""

    def test_decode_submit_response_is_list(self, order_submit_fixture: bytes) -> None:
        """Submit response decodes as a list with one element."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        result = dec.decode(order_submit_fixture)

        assert isinstance(result, list)
        assert len(result) == 1

    def test_submit_order_id_is_uuid_string(self, order_submit_fixture: bytes) -> None:
        """``orderID`` is a UUID string, not an integer."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert isinstance(resp.orderID, str)
        assert "-" in resp.orderID  # UUID format: 8-4-4-4-12

    def test_submit_status_is_inserted(self, order_submit_fixture: bytes) -> None:
        """Submit response has status 2 (ORDER_INSERTED)."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.status == 2

    def test_submit_symbol(self, order_submit_fixture: bytes) -> None:
        """Symbol is decoded correctly."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.symbol == "BTC-EUR"

    def test_submit_side_is_buy(self, order_submit_fixture: bytes) -> None:
        """Side is decoded as 'BUY'."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.side == "BUY"

    def test_submit_order_type_is_integer(self, order_submit_fixture: bytes) -> None:
        """``orderType`` is an integer (76 = LIMIT), not a string."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert isinstance(resp.orderType, int)
        assert resp.orderType == 76

    def test_submit_fill_size_field(self, order_submit_fixture: bytes) -> None:
        """``fillSize`` (not ``filledSize``) is the field name for filled quantity."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.fillSize == 0.0  # unfilled at submission

    def test_submit_timestamp_is_milliseconds(self, order_submit_fixture: bytes) -> None:
        """Timestamp is epoch milliseconds (13-digit integer)."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.timestamp > 1_700_000_000_000  # after 2023 in ms

    def test_cancel_status_is_cancelled(self, order_cancel_fixture: bytes) -> None:
        """Cancel response has status 6 (ORDER_CANCELLED)."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_cancel_fixture)[0]

        assert resp.status == 6

    def test_cancel_preserves_order_id(self, order_cancel_fixture: bytes) -> None:
        """Cancel response echoes the same ``orderID`` as the submit response."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        submit_dec = msgspec.json.Decoder(list[LmexOrderResponse])

        cancel = dec.decode(order_cancel_fixture)[0]
        assert cancel.orderID == "8683ec74-260a-4f27-85db-56c07ca7418e"

    def test_cl_order_id_defaults_to_none(self, order_submit_fixture: bytes) -> None:
        """``clOrderID`` is ``None`` when not supplied in the request."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.clOrderID is None

    def test_stop_price_nullable(self, order_submit_fixture: bytes) -> None:
        """``stopPrice`` field accepts JSON null."""
        dec = msgspec.json.Decoder(list[LmexOrderResponse])
        resp = dec.decode(order_submit_fixture)[0]

        assert resp.stopPrice is None


class TestLmexOpenOrderSchema:
    """Tests for ``LmexOpenOrder`` (open orders endpoint)."""

    def test_decode_open_orders_list(self, open_orders_fixture: bytes) -> None:
        """Open orders decodes as a list."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        result = dec.decode(open_orders_fixture)

        assert isinstance(result, list)
        assert len(result) == 1

    def test_open_order_id_is_uuid_string(self, open_orders_fixture: bytes) -> None:
        """``orderID`` is a UUID string."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert isinstance(order.orderID, str)
        assert "-" in order.orderID

    def test_open_order_symbol(self, open_orders_fixture: bytes) -> None:
        """Symbol decodes correctly."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert order.symbol == "BTC-EUR"

    def test_open_order_state_string(self, open_orders_fixture: bytes) -> None:
        """``orderState`` is a string, not an integer status code."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert order.orderState == "STATUS_ACTIVE"

    def test_open_order_time_in_force_camel_case(self, open_orders_fixture: bytes) -> None:
        """``timeInForce`` uses camelCase (not ``time_in_force``)."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert order.timeInForce == "GTC"

    def test_open_order_filled_size_zero(self, open_orders_fixture: bytes) -> None:
        """``filledSize`` is 0.0 for an unfilled resting order."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert order.filledSize == 0.0

    def test_open_order_cl_order_id_nullable(self, open_orders_fixture: bytes) -> None:
        """``clOrderID`` is ``None`` when absent from the response."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        order = dec.decode(open_orders_fixture)[0]

        assert order.clOrderID is None

    def test_empty_open_orders_returns_empty_list(self) -> None:
        """An empty JSON array decodes to an empty list."""
        dec = msgspec.json.Decoder(list[LmexOpenOrder])
        result = dec.decode(b"[]")
        assert result == []


class TestLmexFillSchema:
    """Tests for ``LmexFill`` (trade history endpoint)."""

    def test_decode_trade_history_list(self, trade_history_fixture: bytes) -> None:
        """Trade history decodes as a list."""
        dec = msgspec.json.Decoder(list[LmexFill])
        result = dec.decode(trade_history_fixture)

        assert isinstance(result, list)
        assert len(result) == 2

    def test_fill_trade_id_is_uuid_string(self, trade_history_fixture: bytes) -> None:
        """``tradeId`` is a UUID string."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert isinstance(fill.tradeId, str)
        assert "-" in fill.tradeId

    def test_fill_order_id_lowercase_d(self, trade_history_fixture: bytes) -> None:
        """
        Fill uses ``orderId`` with a lowercase 'd'.

        This is an inconsistency in the LMEX API — submit/cancel responses use
        ``orderID`` (capital D).
        """
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert isinstance(fill.orderId, str)
        assert "-" in fill.orderId

    def test_fill_filled_price_field(self, trade_history_fixture: bytes) -> None:
        """``filledPrice`` is the execution price field (not ``price``)."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert fill.filledPrice == pytest.approx(69008.82226416, rel=1e-8)

    def test_fill_filled_size_field(self, trade_history_fixture: bytes) -> None:
        """``filledSize`` is the quantity actually executed."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert fill.filledSize == pytest.approx(2e-05, rel=1e-6)

    def test_fill_fee_currency(self, trade_history_fixture: bytes) -> None:
        """Fee currency is decoded correctly."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert fill.feeCurrency == "EUR"

    def test_fill_fee_amount(self, trade_history_fixture: bytes) -> None:
        """Fee amount is a positive float."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert fill.feeAmount > 0

    def test_fill_serial_id_is_integer(self, trade_history_fixture: bytes) -> None:
        """``serialId`` is an integer (used as trade identifier)."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert isinstance(fill.serialId, int)
        assert fill.serialId == 996285327

    def test_fill_side_values(self, trade_history_fixture: bytes) -> None:
        """Sides are 'SELL' and 'BUY' in the fixture."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fills = dec.decode(trade_history_fixture)

        assert fills[0].side == "SELL"
        assert fills[1].side == "BUY"

    def test_fill_timestamp_is_milliseconds(self, trade_history_fixture: bytes) -> None:
        """Timestamp is epoch milliseconds (after 2023)."""
        dec = msgspec.json.Decoder(list[LmexFill])
        fill = dec.decode(trade_history_fixture)[0]

        assert fill.timestamp > 1_700_000_000_000


class TestLmexWalletEntrySchema:
    """Tests for ``LmexWalletEntry`` (wallet endpoint)."""

    def test_decode_wallet_list(self, wallet_fixture: bytes) -> None:
        """Wallet decodes as a list of currency entries."""
        dec = msgspec.json.Decoder(list[LmexWalletEntry])
        result = dec.decode(wallet_fixture)

        assert isinstance(result, list)
        assert len(result) > 0

    def test_wallet_tusd_balance(self, wallet_fixture: bytes) -> None:
        """TUSD (sandbox test currency) has 100,000 balance."""
        dec = msgspec.json.Decoder(list[LmexWalletEntry])
        entries = dec.decode(wallet_fixture)

        tusd = next((e for e in entries if e.currency == "TUSD"), None)
        assert tusd is not None
        assert tusd.total == pytest.approx(100000.0)
        assert tusd.available == pytest.approx(100000.0)

    def test_wallet_eur_balance(self, wallet_fixture: bytes) -> None:
        """EUR balance reflects sandbox account state."""
        dec = msgspec.json.Decoder(list[LmexWalletEntry])
        entries = dec.decode(wallet_fixture)

        eur = next((e for e in entries if e.currency == "EUR"), None)
        assert eur is not None
        assert eur.total > 0
        assert eur.available <= eur.total

    def test_wallet_zero_balance_entries(self, wallet_fixture: bytes) -> None:
        """Zero-balance entries are present (USD = 0 in sandbox)."""
        dec = msgspec.json.Decoder(list[LmexWalletEntry])
        entries = dec.decode(wallet_fixture)

        usd = next((e for e in entries if e.currency == "USD"), None)
        assert usd is not None
        assert usd.total == 0.0
