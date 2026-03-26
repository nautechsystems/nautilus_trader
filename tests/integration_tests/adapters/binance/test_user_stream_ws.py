from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.common.enums import BinancePrivateApiFamily
from nautilus_trader.adapters.binance.common.schemas.user import BinanceListenToken
from nautilus_trader.adapters.binance.http.client import BinanceHttpClient
from nautilus_trader.adapters.binance.http.user import BinanceUserDataHttpAPI
from nautilus_trader.adapters.binance.websocket.user import BinanceUserDataWebSocketClient
from nautilus_trader.common.component import LiveClock


@pytest.mark.asyncio
async def test_margin_user_stream_subscribe_uses_listen_token(event_loop):
    clock = LiveClock()
    http_client = BinanceHttpClient(
        clock=clock,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        base_url="https://api.binance.com/",
    )
    client = BinanceUserDataWebSocketClient(
        clock=clock,
        base_url="wss://ws-api.binance.com:443/ws-api/v3",
        handler=lambda _raw: None,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        loop=event_loop,
        http_client=http_client,
        account_type=BinanceAccountType.MARGIN,
    )
    client._http_user.create_listen_token = AsyncMock(  # type: ignore[union-attr]
        return_value=BinanceListenToken(token="listen-token-1", expirationTime=123456),
    )
    client._send_request = AsyncMock(
        return_value={"result": {"subscriptionId": 7, "expirationTime": 234567}},
    )

    await client.session_logon()
    subscription_id = await client.subscribe_user_data_stream()

    assert client.is_authenticated is True
    assert subscription_id == "7"
    client._send_request.assert_awaited_once_with(  # type: ignore[attr-defined]
        "userDataStream.subscribe.listenToken",
        {"listenToken": "listen-token-1"},
    )
    client._cancel_keepalive()


@pytest.mark.asyncio
async def test_margin_user_stream_renew_and_unsubscribe_use_current_subscription(event_loop):
    clock = LiveClock()
    http_client = BinanceHttpClient(
        clock=clock,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        base_url="https://api.binance.com/",
    )
    client = BinanceUserDataWebSocketClient(
        clock=clock,
        base_url="wss://ws-api.binance.com:443/ws-api/v3",
        handler=lambda _raw: None,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        loop=event_loop,
        http_client=http_client,
        account_type=BinanceAccountType.MARGIN,
    )
    client._http_user.create_listen_token = AsyncMock(  # type: ignore[union-attr]
        side_effect=[
            BinanceListenToken(token="listen-token-1", expirationTime=123456),
            BinanceListenToken(token="listen-token-2", expirationTime=345678),
        ],
    )
    client._send_request = AsyncMock(
        side_effect=[
            {"result": {"subscriptionId": 7, "expirationTime": 234567}},
            {"result": {"subscriptionId": 8, "expirationTime": 456789}},
            {"result": {}},
        ],
    )

    await client.session_logon()
    await client.subscribe_user_data_stream()
    await client._renew_margin_subscription()
    await client.unsubscribe_user_data_stream()

    assert client._subscription_expiration_time_ms == 456789
    client._send_request.assert_any_await(  # type: ignore[attr-defined]
        "userDataStream.subscribe.listenToken",
        {"listenToken": "listen-token-2"},
    )
    client._send_request.assert_any_await(  # type: ignore[attr-defined]
        "userDataStream.unsubscribe",
        {"subscriptionId": "8"},
    )


def test_portfolio_margin_futures_user_stream_uses_papi_listen_key_endpoint():
    clock = LiveClock()
    http_client = BinanceHttpClient(
        clock=clock,
        api_key="SOME_BINANCE_API_KEY",
        api_secret="SOME_BINANCE_API_SECRET",
        base_url="https://papi.binance.com/",
    )

    api = BinanceUserDataHttpAPI(
        client=http_client,
        account_type=BinanceAccountType.USDT_FUTURES,
        private_api_family=BinancePrivateApiFamily.PORTFOLIO_MARGIN,
    )

    assert api._endpoint_listenkey is not None
    assert api._endpoint_listenkey.url_path == "/papi/v1/listenKey"
