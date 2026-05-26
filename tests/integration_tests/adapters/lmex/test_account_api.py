# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Integration tests for ``LmexAccountHttpAPI``.

These tests verify the full decode chain from raw HTTP bytes through to typed
Python objects.  The ``LmexHttpClient`` is replaced by a mock that returns
pre-recorded fixture bytes from ``tests/resources/http_responses/``.

No live API calls are made.
"""

from __future__ import annotations

import pytest

from nautilus_trader.adapters.lmex.http.account import LmexAccountHttpAPI
from nautilus_trader.adapters.lmex.schemas.order import (
    LmexFill,
    LmexOpenOrder,
    LmexOrderResponse,
    LmexWalletEntry,
)


class TestLmexAccountHttpAPISubmitOrder:
    """Integration tests for ``LmexAccountHttpAPI.submit_order``."""

    @pytest.mark.asyncio
    async def test_submit_order_returns_order_response(
        self, mock_http_client
    ) -> None:
        """``submit_order`` returns a single ``LmexOrderResponse`` (first list element)."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
        )

        assert isinstance(result, LmexOrderResponse)

    @pytest.mark.asyncio
    async def test_submit_order_status_inserted(self, mock_http_client) -> None:
        """Submitted order has status 2 (ORDER_INSERTED / ACCEPTED)."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
        )

        assert result.status == 2

    @pytest.mark.asyncio
    async def test_submit_order_id_is_uuid(self, mock_http_client) -> None:
        """``orderID`` on the returned response is a UUID string."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
        )

        assert isinstance(result.orderID, str)
        assert "-" in result.orderID

    @pytest.mark.asyncio
    async def test_submit_order_posts_to_correct_path(
        self, mock_http_client
    ) -> None:
        """``submit_order`` calls POST on ``/api/v3.2/order``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
        )

        mock_http_client.post.assert_called_once()
        call_args = mock_http_client.post.call_args
        assert call_args[0][0] == "/api/v3.2/order"

    @pytest.mark.asyncio
    async def test_submit_order_payload_contains_symbol(
        self, mock_http_client
    ) -> None:
        """The POST payload includes the symbol."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
        )

        payload = mock_http_client.post.call_args.kwargs.get("payload") or \
                  mock_http_client.post.call_args[1].get("payload") or \
                  mock_http_client.post.call_args[0][1] if len(mock_http_client.post.call_args[0]) > 1 else None
        # Extract from kwargs if present
        call_kwargs = mock_http_client.post.call_args.kwargs
        assert "payload" in call_kwargs
        assert call_kwargs["payload"]["symbol"] == "BTC-EUR"

    @pytest.mark.asyncio
    async def test_submit_order_with_client_order_id(
        self, mock_http_client
    ) -> None:
        """``clOrderId`` key is included in payload when ``client_order_id`` is given."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.submit_order(
            symbol="BTC-EUR",
            side="BUY",
            order_type="LIMIT",
            size=0.00005,
            price=1.0,
            client_order_id="my-order-abc",
        )

        payload = mock_http_client.post.call_args.kwargs["payload"]
        assert payload.get("clOrderId") == "my-order-abc"

    @pytest.mark.asyncio
    async def test_submit_market_order_omits_price(
        self, mock_http_client
    ) -> None:
        """A MARKET order submission does not include a price in the payload."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.submit_order(
            symbol="BTC-EUR",
            side="SELL",
            order_type="MARKET",
            size=0.00005,
            price=None,
        )

        payload = mock_http_client.post.call_args.kwargs["payload"]
        assert "price" not in payload


class TestLmexAccountHttpAPICancelOrder:
    """Integration tests for ``LmexAccountHttpAPI.cancel_order``."""

    @pytest.mark.asyncio
    async def test_cancel_order_returns_order_response(
        self, mock_http_client
    ) -> None:
        """``cancel_order`` returns a ``LmexOrderResponse``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.cancel_order(
            symbol="BTC-EUR",
            order_id="8683ec74-260a-4f27-85db-56c07ca7418e",
        )

        assert isinstance(result, LmexOrderResponse)

    @pytest.mark.asyncio
    async def test_cancel_order_status_cancelled(self, mock_http_client) -> None:
        """Cancel response has status 6 (ORDER_CANCELLED)."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.cancel_order(
            symbol="BTC-EUR",
            order_id="8683ec74-260a-4f27-85db-56c07ca7418e",
        )

        assert result.status == 6

    @pytest.mark.asyncio
    async def test_cancel_order_uses_query_params(self, mock_http_client) -> None:
        """Cancel sends ``orderID`` as a query parameter, not a JSON body."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.cancel_order(
            symbol="BTC-EUR",
            order_id="8683ec74-260a-4f27-85db-56c07ca7418e",
        )

        mock_http_client.delete.assert_called_once()
        call_kwargs = mock_http_client.delete.call_args.kwargs
        assert "params" in call_kwargs
        assert call_kwargs["params"]["orderID"] == "8683ec74-260a-4f27-85db-56c07ca7418e"
        assert call_kwargs["params"]["symbol"] == "BTC-EUR"

    @pytest.mark.asyncio
    async def test_cancel_order_raises_without_ids(
        self, mock_http_client
    ) -> None:
        """``cancel_order`` raises ``ValueError`` when both IDs are absent."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        with pytest.raises(ValueError, match="order_id or client_order_id"):
            await api.cancel_order(symbol="BTC-EUR", order_id="")

    @pytest.mark.asyncio
    async def test_cancel_order_by_client_id(self, mock_http_client) -> None:
        """When ``order_id`` is empty, ``clOrderID`` is used instead."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.cancel_order(
            symbol="BTC-EUR",
            order_id="",
            client_order_id="my-cl-id",
        )

        call_kwargs = mock_http_client.delete.call_args.kwargs
        assert call_kwargs["params"].get("clOrderID") == "my-cl-id"


class TestLmexAccountHttpAPIOpenOrders:
    """Integration tests for ``LmexAccountHttpAPI.get_open_orders``."""

    @pytest.mark.asyncio
    async def test_get_open_orders_returns_list(self, mock_http_client) -> None:
        """``get_open_orders`` returns a list of ``LmexOpenOrder``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_open_orders(symbol="BTC-EUR")

        assert isinstance(result, list)
        assert len(result) == 1
        assert isinstance(result[0], LmexOpenOrder)

    @pytest.mark.asyncio
    async def test_get_open_orders_order_id_uuid(
        self, mock_http_client
    ) -> None:
        """Open order ``orderID`` is a UUID string."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_open_orders(symbol="BTC-EUR")

        assert "-" in result[0].orderID

    @pytest.mark.asyncio
    async def test_get_open_orders_state_active(self, mock_http_client) -> None:
        """Resting order has ``orderState == 'STATUS_ACTIVE'``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_open_orders(symbol="BTC-EUR")

        assert result[0].orderState == "STATUS_ACTIVE"

    @pytest.mark.asyncio
    async def test_get_open_orders_uses_signed_get(
        self, mock_http_client
    ) -> None:
        """``get_open_orders`` calls the HTTP client with ``signed=True``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.get_open_orders(symbol="BTC-EUR")

        call_kwargs = mock_http_client.get.call_args.kwargs
        assert call_kwargs.get("signed") is True

    @pytest.mark.asyncio
    async def test_get_open_orders_without_symbol_passes_none_params(
        self, mock_http_client
    ) -> None:
        """When no symbol is given, no ``params`` are sent."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.get_open_orders()

        call_kwargs = mock_http_client.get.call_args.kwargs
        assert call_kwargs.get("params") is None


class TestLmexAccountHttpAPIFills:
    """Integration tests for ``LmexAccountHttpAPI.get_fills``."""

    @pytest.mark.asyncio
    async def test_get_fills_returns_list(self, mock_http_client) -> None:
        """``get_fills`` returns a list of ``LmexFill``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_fills(symbol="BTC-EUR")

        assert isinstance(result, list)
        assert len(result) == 2
        assert isinstance(result[0], LmexFill)

    @pytest.mark.asyncio
    async def test_get_fills_trade_id_uuid(self, mock_http_client) -> None:
        """Fill ``tradeId`` is a UUID string."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_fills(symbol="BTC-EUR")

        assert "-" in result[0].tradeId

    @pytest.mark.asyncio
    async def test_get_fills_filled_price(self, mock_http_client) -> None:
        """``filledPrice`` is the execution price."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_fills(symbol="BTC-EUR")

        assert result[0].filledPrice == pytest.approx(69008.82226416, rel=1e-8)

    @pytest.mark.asyncio
    async def test_get_fills_fee_currency(self, mock_http_client) -> None:
        """Fee currency is decoded correctly."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_fills(symbol="BTC-EUR")

        assert result[0].feeCurrency == "EUR"

    @pytest.mark.asyncio
    async def test_get_fills_with_count_param(self, mock_http_client) -> None:
        """``count`` parameter is forwarded in the GET request."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.get_fills(symbol="BTC-EUR", count=10)

        call_kwargs = mock_http_client.get.call_args.kwargs
        assert call_kwargs["params"]["count"] == 10


class TestLmexAccountHttpAPIWallet:
    """Integration tests for ``LmexAccountHttpAPI.get_wallet_balance``."""

    @pytest.mark.asyncio
    async def test_get_wallet_balance_returns_list(
        self, mock_http_client
    ) -> None:
        """``get_wallet_balance`` returns a list of ``LmexWalletEntry``."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_wallet_balance()

        assert isinstance(result, list)
        assert all(isinstance(e, LmexWalletEntry) for e in result)

    @pytest.mark.asyncio
    async def test_get_wallet_tusd_present(self, mock_http_client) -> None:
        """Sandbox TUSD balance is in the wallet response."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_wallet_balance()

        tusd = next((e for e in result if e.currency == "TUSD"), None)
        assert tusd is not None
        assert tusd.total == pytest.approx(100000.0)

    @pytest.mark.asyncio
    async def test_get_wallet_uses_signed_get(self, mock_http_client) -> None:
        """Wallet request is signed (authenticated)."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.get_wallet_balance()

        call_kwargs = mock_http_client.get.call_args.kwargs
        assert call_kwargs.get("signed") is True

    @pytest.mark.asyncio
    async def test_get_wallet_balance_non_zero_entries(
        self, mock_http_client
    ) -> None:
        """At least one balance entry has a non-zero total."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        result = await api.get_wallet_balance()

        non_zero = [e for e in result if e.total > 0]
        assert len(non_zero) >= 1


class TestLmexAccountHttpAPICancelAll:
    """Integration tests for ``LmexAccountHttpAPI.cancel_all_orders``."""

    @pytest.mark.asyncio
    async def test_cancel_all_iterates_open_orders(
        self, mock_http_client
    ) -> None:
        """``cancel_all_orders`` fetches open orders then cancels each one."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        results = await api.cancel_all_orders(symbol="BTC-EUR")

        # One open order in fixture → one cancel call
        assert len(results) == 1
        assert results[0].status == 6

    @pytest.mark.asyncio
    async def test_cancel_all_uses_correct_order_id(
        self, mock_http_client
    ) -> None:
        """Cancel-all passes the UUID from the open order response."""
        api = LmexAccountHttpAPI(client=mock_http_client)

        await api.cancel_all_orders(symbol="BTC-EUR")

        # The delete call should contain the open order's UUID
        delete_kwargs = mock_http_client.delete.call_args.kwargs
        assert delete_kwargs["params"]["orderID"] == "71d05160-dff4-4d7c-80f1-2803657ee408"
