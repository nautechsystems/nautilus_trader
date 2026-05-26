# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Shared pytest fixtures for the LMEX adapter test suite.

All fixtures load JSON from ``tests/resources/`` — no live API calls are made.
"""

from __future__ import annotations

from pathlib import Path
from unittest.mock import AsyncMock, MagicMock

import pytest

# ---------------------------------------------------------------------------
# Paths
# ---------------------------------------------------------------------------

RESOURCES = Path(__file__).parent / "resources"
HTTP = RESOURCES / "http_responses"
WS = RESOURCES / "ws_messages"


def _load(path: Path) -> bytes:
    return path.read_bytes()


# ---------------------------------------------------------------------------
# HTTP response fixtures
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def time_fixture() -> bytes:
    return _load(HTTP / "time.json")


@pytest.fixture(scope="session")
def orderbook_fixture() -> bytes:
    return _load(HTTP / "orderbook_btcusd.json")


@pytest.fixture(scope="session")
def trades_fixture() -> bytes:
    return _load(HTTP / "trades_btcusd.json")


@pytest.fixture(scope="session")
def market_summary_fixture() -> bytes:
    return _load(HTTP / "market_summary_sample.json")


@pytest.fixture(scope="session")
def order_submit_fixture() -> bytes:
    return _load(HTTP / "order_submit_btceur.json")


@pytest.fixture(scope="session")
def order_cancel_fixture() -> bytes:
    return _load(HTTP / "order_cancel_btceur.json")


@pytest.fixture(scope="session")
def open_orders_fixture() -> bytes:
    return _load(HTTP / "open_orders_btceur.json")


@pytest.fixture(scope="session")
def trade_history_fixture() -> bytes:
    return _load(HTTP / "trade_history_btceur.json")


@pytest.fixture(scope="session")
def wallet_fixture() -> bytes:
    return _load(HTTP / "wallet.json")


# ---------------------------------------------------------------------------
# WebSocket message fixtures
# ---------------------------------------------------------------------------

@pytest.fixture(scope="session")
def ws_trade_msg() -> bytes:
    return _load(WS / "trade_feed.json")


@pytest.fixture(scope="session")
def ws_subscribe_ack() -> bytes:
    return _load(WS / "subscribe_ack.json")


@pytest.fixture(scope="session")
def ws_order_fill() -> bytes:
    return _load(WS / "order_event_fill.json")


@pytest.fixture(scope="session")
def ws_order_cancel() -> bytes:
    return _load(WS / "order_event_cancel.json")


# ---------------------------------------------------------------------------
# Mock HTTP client
# ---------------------------------------------------------------------------

@pytest.fixture
def mock_http_client(
    orderbook_fixture,
    trades_fixture,
    market_summary_fixture,
    time_fixture,
    order_submit_fixture,
    order_cancel_fixture,
    open_orders_fixture,
    trade_history_fixture,
    wallet_fixture,
):
    """
    A mock ``LmexHttpClient`` with pre-configured responses for all endpoints.
    """
    client = MagicMock()
    client.api_key = "TEST_API_KEY"
    client.api_key_masked = "TEST...EKEY"

    async def _get(path, params=None, signed=False):
        if "/time" in path:
            return time_fixture
        if "/orderbook" in path:
            return orderbook_fixture
        if "/trades" in path:
            return trades_fixture
        if "/market_summary" in path:
            return market_summary_fixture
        if "/user/wallet" in path:
            return wallet_fixture
        if "/user/open_orders" in path:
            return open_orders_fixture
        if "/user/trade_history" in path:
            return trade_history_fixture
        raise ValueError(f"Unmocked GET path: {path}")

    async def _post(path, payload=None):
        if "/order" in path:
            return order_submit_fixture
        raise ValueError(f"Unmocked POST path: {path}")

    async def _delete(path, params=None, payload=None):
        if "/order" in path:
            return order_cancel_fixture
        raise ValueError(f"Unmocked DELETE path: {path}")

    client.get = AsyncMock(side_effect=_get)
    client.post = AsyncMock(side_effect=_post)
    client.delete = AsyncMock(side_effect=_delete)

    return client
