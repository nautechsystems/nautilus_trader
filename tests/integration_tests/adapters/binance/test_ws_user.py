# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
from unittest.mock import AsyncMock
from unittest.mock import MagicMock

import msgspec
import pytest

from nautilus_trader.adapters.binance.common.enums import BinanceAccountType
from nautilus_trader.adapters.binance.websocket import user as user_module
from nautilus_trader.adapters.binance.websocket.user import BinanceUserDataWebSocketClient
from nautilus_trader.common.component import LiveClock


@pytest.fixture
def event_loop():
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


def _make_client(
    event_loop: asyncio.AbstractEventLoop,
    *,
    is_futures: bool = True,
    http_client=None,
    on_resubscribe=None,
) -> BinanceUserDataWebSocketClient:
    return BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=lambda _: None,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=is_futures,
        stream_base_url="wss://example.invalid/private",
        is_ed25519=False,
        http_client=http_client,
        on_resubscribe=on_resubscribe,
    )


def test_ws_api_reconnecting_returns_false_when_client_not_set(event_loop):
    client = _make_client(event_loop)
    client._client = None

    assert client._ws_api_reconnecting() is False


@pytest.mark.parametrize(
    ("account_type", "stream_base_url", "expected"),
    [
        (
            BinanceAccountType.USDT_FUTURES,
            "wss://fstream.binance.com/private",
            "wss://fstream.binance.com/private/ws?listenKey=redacted",
        ),
        (
            BinanceAccountType.COIN_FUTURES,
            "wss://dstream.binancefuture.com/ws",
            "wss://dstream.binancefuture.com/ws/redacted",
        ),
    ],
)
def test_connect_stream_uses_product_specific_listen_key_url(
    event_loop,
    monkeypatch,
    account_type,
    stream_base_url,
    expected,
):
    connect = AsyncMock(return_value=MagicMock())
    websocket_client = MagicMock(connect=connect)
    monkeypatch.setattr(user_module, "WebSocketClient", websocket_client)
    monkeypatch.setattr(user_module, "WebSocketConfig", lambda **kwargs: kwargs)

    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=lambda _: None,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=True,
        stream_base_url=stream_base_url,
        is_ed25519=False,
        account_type=account_type,
    )

    event_loop.run_until_complete(client._connect_stream("redacted"))

    assert connect.await_args.kwargs["config"]["url"] == expected


def test_send_request_logs_method_without_sensitive_payload(event_loop):
    client = _make_client(event_loop)
    client._log = MagicMock()
    client._client = MagicMock()

    async def send_text(raw):
        request = msgspec.json.decode(raw)
        response = {
            "id": request["id"],
            "result": {"listenKey": "redacted"},
        }
        client._handle_message(msgspec.json.encode(response))

    client._client.send_text = send_text

    response = event_loop.run_until_complete(
        client._send_request(
            "userDataStream.start",
            {"apiKey": "redacted", "listenKey": "redacted"},
        ),
    )
    logged_messages = [call.args[0] for call in client._log.debug.call_args_list]

    assert response == {"id": "0", "result": {"listenKey": "redacted"}}
    assert logged_messages == [
        "SENDING: id=0, method=userDataStream.start",
        "RECEIVED: id=0, method=userDataStream.start",
    ]


def test_handle_message_diagnostics_omit_sensitive_payload(event_loop):
    client = _make_client(event_loop)
    client._log = MagicMock()

    client._handle_message(b'{"listenKey":"redacted"')
    client._handle_message(
        msgspec.json.encode(
            {"id": "0", "result": {"listenKey": "redacted"}},
        ),
    )

    client._log.error.assert_called_once_with("Failed to decode WebSocket API message")
    client._log.warning.assert_called_once_with("Unhandled WebSocket API message")


def test_ws_api_reconnecting_returns_false_when_client_idle(event_loop):
    client = _make_client(event_loop)
    mock_ws = MagicMock()
    mock_ws.is_reconnecting.return_value = False
    mock_ws.is_disconnecting.return_value = False
    client._client = mock_ws

    assert client._ws_api_reconnecting() is False


def test_ws_api_reconnecting_returns_true_when_socket_reconnecting(event_loop):
    client = _make_client(event_loop)
    mock_ws = MagicMock()
    mock_ws.is_reconnecting.return_value = True
    mock_ws.is_disconnecting.return_value = False
    client._client = mock_ws

    assert client._ws_api_reconnecting() is True


def test_ws_api_reconnecting_returns_true_when_socket_disconnecting(event_loop):
    client = _make_client(event_loop)
    mock_ws = MagicMock()
    mock_ws.is_reconnecting.return_value = False
    mock_ws.is_disconnecting.return_value = True
    client._client = mock_ws

    assert client._ws_api_reconnecting() is True


def test_ws_api_reconnecting_returns_true_when_reconnect_task_pending(event_loop):
    # The bug the review caught: socket finished its TCP handshake but the
    # queued _reauth_and_resubscribe has not yet run, so a concurrent
    # _resubscribe must still defer.
    client = _make_client(event_loop)
    mock_ws = MagicMock()
    mock_ws.is_reconnecting.return_value = False
    mock_ws.is_disconnecting.return_value = False
    client._client = mock_ws

    pending_task = MagicMock(spec=asyncio.Task)
    pending_task.done.return_value = False
    client._reconnect_task = pending_task

    assert client._ws_api_reconnecting() is True


def test_ws_api_reconnecting_returns_false_when_reconnect_task_done(event_loop):
    client = _make_client(event_loop)
    mock_ws = MagicMock()
    mock_ws.is_reconnecting.return_value = False
    mock_ws.is_disconnecting.return_value = False
    client._client = mock_ws

    completed_task = MagicMock(spec=asyncio.Task)
    completed_task.done.return_value = True
    client._reconnect_task = completed_task

    assert client._ws_api_reconnecting() is False


def test_safe_pre_dispatch_hook_awaits_callback(event_loop):
    call_count = 0

    async def callback():
        nonlocal call_count
        call_count += 1

    client = _make_client(event_loop)

    event_loop.run_until_complete(client._safe_pre_dispatch_hook(callback))

    assert call_count == 1


def test_safe_pre_dispatch_hook_swallows_callback_exception(event_loop):
    async def failing_callback():
        raise RuntimeError("reconcile failed")

    client = _make_client(event_loop)

    # Must not raise; a failure in reconciliation should not bring down
    # the recovery path.
    event_loop.run_until_complete(client._safe_pre_dispatch_hook(failing_callback))


def test_stream_message_buffers_while_dispatch_paused(event_loop):
    # Stream events arriving while dispatch is paused (during recovery
    # reconciliation) must not be delivered to the handler yet.
    delivered: list[bytes] = []
    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=delivered.append,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=True,
        stream_base_url="wss://example.invalid/private",
        is_ed25519=False,
    )

    client._dispatch_paused = True
    client._handle_stream_message(b'{"event":"fresh1"}')
    client._handle_stream_message(b'{"event":"fresh2"}')

    assert delivered == []
    assert client._dispatch_buffer == [b'{"event":"fresh1"}', b'{"event":"fresh2"}']


def test_resume_dispatch_drains_buffer_in_order(event_loop):
    delivered: list[bytes] = []
    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=delivered.append,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=True,
        stream_base_url="wss://example.invalid/private",
        is_ed25519=False,
    )

    client._dispatch_paused = True
    client._handle_stream_message(b"event1")
    client._handle_stream_message(b"event2")

    client._resume_dispatch()

    assert delivered == [b"event1", b"event2"]
    assert client._dispatch_buffer == []
    assert client._dispatch_paused is False

    # Subsequent events dispatch live, not buffered
    client._handle_stream_message(b"event3")
    assert delivered == [b"event1", b"event2", b"event3"]


def test_spot_inline_event_buffers_while_dispatch_paused(event_loop):
    # Spot user-data events arrive inline via _handle_message, which has its
    # own pause check. A mutation dropping that check on the Spot path would
    # not be caught by the futures-side _handle_stream_message tests.
    import msgspec

    delivered: list[bytes] = []
    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=delivered.append,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=False,
    )

    spot_event = {"subscriptionId": 1, "event": {"e": "executionReport", "s": "BTCUSDT"}}
    raw = msgspec.json.encode(spot_event)
    expected_payload = msgspec.json.encode(spot_event["event"])

    client._dispatch_paused = True
    client._handle_message(raw)

    assert delivered == []
    assert client._dispatch_buffer == [expected_payload]

    client._resume_dispatch()

    assert delivered == [expected_payload]
    assert client._dispatch_buffer == []


def test_subscribe_user_data_stream_hook_runs_before_buffered_events_drain(event_loop):
    # End-to-end invariant: during resubscribe, the pre_dispatch_hook (mass
    # status reconciliation) must run to completion before any fresh stream
    # event is handed to the nautilus event pipeline. Achieved via connect
    # stream -> pause dispatch -> run hook -> drain buffered events.
    delivered: list[bytes] = []
    order_log: list[str] = []

    client = BinanceUserDataWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=delivered.append,
        api_key="test-api-key",
        api_secret="test-api-secret",
        loop=event_loop,
        is_futures=True,
        stream_base_url="wss://example.invalid/private",
        is_ed25519=False,
    )
    client._is_authenticated = True

    # Fake the WS API listenKey response
    async def fake_send_request(method, params=None, timeout=10.0):
        assert method == "userDataStream.start"
        return {"result": {"listenKey": "test-listen-key"}}

    # When _connect_stream is called, simulate an event arriving on the new
    # stream before the hook finishes. _handle_stream_message must queue it
    # because _dispatch_paused is True at that point.
    async def fake_connect_stream(listen_key: str):
        order_log.append("connected")
        assert client._dispatch_paused is True
        client._handle_stream_message(b'{"event":"fresh"}')

    # Replace the keepalive coroutine so the spawned task completes instantly
    async def fake_keepalive():
        return

    client._send_request = fake_send_request
    client._connect_stream = fake_connect_stream
    client._keepalive_loop = fake_keepalive

    async def hook():
        # The fresh event from fake_connect_stream was buffered and not yet
        # delivered to the handler while this hook runs.
        assert delivered == []
        assert client._dispatch_buffer == [b'{"event":"fresh"}']
        order_log.append("hook")

    event_loop.run_until_complete(client.subscribe_user_data_stream(pre_dispatch_hook=hook))

    assert order_log == ["connected", "hook"]
    assert delivered == [b'{"event":"fresh"}']
    assert client._dispatch_paused is False
    assert client._dispatch_buffer == []
