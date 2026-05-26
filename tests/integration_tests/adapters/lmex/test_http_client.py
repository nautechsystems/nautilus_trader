# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for ``LmexHttpClient`` request construction and error handling.

No live API calls are made; the ``nautilus_pyo3.HttpClient`` is mocked.
"""

from __future__ import annotations

import hashlib
import hmac
from unittest.mock import AsyncMock, MagicMock, patch

import pytest


class TestLmexHttpClientAuth:
    """Tests for authentication header generation."""

    def _make_client(self, api_key: str = "MYKEY", api_secret: str = "MYSECRET"):
        """
        Create a minimal ``LmexHttpClient`` with mocked dependencies.
        """
        from nautilus_trader.adapters.lmex.http.client import LmexHttpClient

        clock = MagicMock()
        with patch("nautilus_trader.adapters.lmex.http.client.HttpClient"):
            client = LmexHttpClient(
                clock=clock,
                api_key=api_key,
                api_secret=api_secret,
                base_url="https://api.lmex.io/spot",
            )
        return client

    def test_sign_produces_sha384_hex(self) -> None:
        """``_sign`` returns a lowercase 96-char hex string (SHA-384 output)."""
        client = self._make_client()
        sig = client._sign("/spot/api/v3.2/order", "1779786400000", '{"x":1}')
        assert isinstance(sig, str)
        assert len(sig) == 96
        assert sig == sig.lower()

    def test_sign_matches_reference(self) -> None:
        """``_sign`` output matches the stdlib hmac reference implementation."""
        secret = "MYSECRET"
        path = "/spot/api/v3.2/order"
        nonce = "1779786400000"
        body = '{"symbol":"BTC-USD"}'

        client = self._make_client(api_secret=secret)
        result = client._sign(path, nonce, body)

        expected = hmac.new(
            secret.encode(),
            (path + nonce + body).encode(),
            hashlib.sha384,
        ).hexdigest()

        assert result == expected

    def test_auth_headers_contain_all_three(self) -> None:
        """Auth headers include request-api, request-nonce, request-sign."""
        client = self._make_client()
        headers = client._auth_headers("/spot/api/v3.2/orderbook", "12345", "")
        assert "request-api" in headers
        assert "request-nonce" in headers
        assert "request-sign" in headers
        assert headers["request-api"] == "MYKEY"
        assert headers["request-nonce"] == "12345"

    def test_sign_raises_without_secret(self) -> None:
        """``_sign`` raises ``ValueError`` when api_secret is None."""
        client = self._make_client(api_key="KEY", api_secret=None)
        client._secret = None  # force None after construction
        with pytest.raises(ValueError, match="api_secret"):
            client._sign("/path", "123", "")

    def test_auth_headers_raise_without_key(self) -> None:
        """``_auth_headers`` raises ``ValueError`` when api_key is None."""
        client = self._make_client()
        client._key = None
        with pytest.raises(ValueError, match="api_key"):
            client._auth_headers("/path", "123", "")

    def test_api_key_masked(self) -> None:
        """``api_key_masked`` returns first-4 + last-4 characters."""
        client = self._make_client(api_key="ABCDEFGH1234WXYZ")
        masked = client.api_key_masked
        assert masked.startswith("ABCD")
        assert masked.endswith("WXYZ")
        assert "..." in masked

    def test_api_key_masked_short(self) -> None:
        """Short API keys are fully redacted."""
        client = self._make_client(api_key="SHORT")
        assert client.api_key_masked == "****"

    def test_api_key_masked_none(self) -> None:
        """No key returns '<none>'."""
        client = self._make_client(api_key=None, api_secret=None)
        client._key = None
        assert client.api_key_masked == "<none>"


class TestLmexHttpClientErrors:
    """Tests for error raising on 4xx / 5xx responses."""

    def _make_client_with_mock_http(self, status: int, body: bytes = b'{"error":"test"}'):
        from nautilus_trader.adapters.lmex.http.client import LmexHttpClient

        clock = MagicMock()
        mock_response = MagicMock()
        mock_response.status = status
        mock_response.body = body
        mock_response.headers = {}

        mock_http = MagicMock()
        mock_http.request = AsyncMock(return_value=mock_response)

        with patch("nautilus_trader.adapters.lmex.http.client.HttpClient", return_value=mock_http):
            client = LmexHttpClient(
                clock=clock,
                api_key="KEY",
                api_secret="SECRET",
                base_url="https://api.lmex.io/spot",
            )
        return client

    @pytest.mark.asyncio
    async def test_400_raises_client_error(self) -> None:
        """HTTP 400 raises ``LmexClientError``."""
        from nautilus_trader.adapters.lmex.http.error import LmexClientError

        client = self._make_client_with_mock_http(400)
        with pytest.raises(LmexClientError) as exc_info:
            await client.get("/api/v3.2/orderbook", params={"symbol": "BTC-USD"})
        assert exc_info.value.status == 400

    @pytest.mark.asyncio
    async def test_500_raises_server_error(self) -> None:
        """HTTP 500 raises ``LmexServerError``."""
        from nautilus_trader.adapters.lmex.http.error import LmexServerError

        client = self._make_client_with_mock_http(500, b"Internal Server Error")
        with pytest.raises(LmexServerError) as exc_info:
            await client.get("/api/v3.2/orderbook")
        assert exc_info.value.status == 500

    @pytest.mark.asyncio
    async def test_200_returns_body(self) -> None:
        """HTTP 200 returns the response body bytes."""
        client = self._make_client_with_mock_http(200, b'{"symbol":"BTC-USD"}')
        result = await client.get("/api/v3.2/orderbook")
        assert result == b'{"symbol":"BTC-USD"}'

    def test_should_retry_server_error(self) -> None:
        """Server errors are retryable; client errors are not."""
        from nautilus_trader.adapters.lmex.http.error import LmexClientError
        from nautilus_trader.adapters.lmex.http.error import LmexServerError
        from nautilus_trader.adapters.lmex.http.error import should_retry

        assert should_retry(LmexServerError(500, "oops", {})) is True
        assert should_retry(LmexClientError(400, "bad", {})) is False
        assert should_retry(ValueError("unrelated")) is False
