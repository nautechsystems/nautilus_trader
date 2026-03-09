import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.websocket.user import BinanceUserDataWebSocketClient
from nautilus_trader.common.component import LiveClock


def _build_margin_client(loop: asyncio.AbstractEventLoop) -> BinanceUserDataWebSocketClient:
    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://ws-api.binance.com:443/ws-api/v3",
        handler=lambda _raw: None,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=loop,
        account_type=BinanceAccountType.MARGIN,
        http_client=SimpleNamespace(
            send_request=AsyncMock(),
            sign_request=AsyncMock(),
        ),
    )
    client._client = AsyncMock()
    return client


@pytest.mark.asyncio
async def test_margin_listen_token_subscription_does_not_require_session_logon():
    client = _build_margin_client(asyncio.get_running_loop())
    client._http_user = SimpleNamespace(
        create_listen_token=AsyncMock(
            return_value=SimpleNamespace(token="listen-token-1", expirationTime=1234567890),
        ),
    )
    client._send_request = AsyncMock(
        return_value={"result": {"subscriptionId": 17, "expirationTime": 1234567890}},
    )
    client._keepalive_loop = AsyncMock()

    subscription_id = await client.subscribe_user_data_stream()

    assert subscription_id == "17"
    assert client.subscription_id == "17"
    client._send_request.assert_awaited_once_with(
        "userDataStream.subscribe.listenToken",
        {"listenToken": "listen-token-1"},
    )
    if client._keepalive_task is not None:
        client._keepalive_task.cancel()
        await asyncio.gather(client._keepalive_task, return_exceptions=True)


@pytest.mark.asyncio
async def test_margin_listen_token_renewal_refreshes_subscription():
    client = _build_margin_client(asyncio.get_running_loop())
    client._http_user = SimpleNamespace(
        create_listen_token=AsyncMock(
            return_value=SimpleNamespace(token="listen-token-2", expirationTime=2234567890),
        ),
    )
    client._send_request = AsyncMock(
        return_value={"result": {"subscriptionId": 18, "expirationTime": 2234567890}},
    )

    await client._renew_margin_subscription()

    assert client.subscription_id == "18"
    client._send_request.assert_awaited_once_with(
        "userDataStream.subscribe.listenToken",
        {"listenToken": "listen-token-2"},
    )


@pytest.mark.asyncio
async def test_margin_unsubscribe_uses_subscription_id():
    client = _build_margin_client(asyncio.get_running_loop())
    client._http_user = SimpleNamespace(create_listen_token=AsyncMock())
    client._send_request = AsyncMock(return_value={"result": {}})
    client._subscription_id = "19"

    await client.unsubscribe_user_data_stream()

    client._send_request.assert_awaited_once_with(
        "userDataStream.unsubscribe",
        {"subscriptionId": "19"},
    )
    assert client.subscription_id is None
