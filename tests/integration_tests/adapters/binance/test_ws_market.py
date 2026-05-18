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

import pytest

from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core.nautilus_pyo3 import WebSocketClientError


@pytest.fixture
def event_loop():
    loop = asyncio.new_event_loop()
    yield loop
    loop.close()


def _make_client(event_loop: asyncio.AbstractEventLoop) -> BinanceWebSocketClient:
    return BinanceWebSocketClient(
        clock=LiveClock(),
        base_url="wss://example.invalid/ws",
        handler=lambda _: None,
        handler_reconnect=None,
        loop=event_loop,
    )


def test_send_pong_swallows_runtime_error_when_connection_not_active(event_loop):
    client = _make_client(event_loop)

    inner = MagicMock()
    inner.send_pong = AsyncMock(
        side_effect=RuntimeError("Cannot send pong: connection not active"),
    )
    client._clients[0] = inner

    # Wrapper must swallow RuntimeError and log a warning rather than re-raise
    event_loop.run_until_complete(client.send_pong(0, b"ping-payload"))

    inner.send_pong.assert_awaited_once_with(b"ping-payload")


def test_send_pong_swallows_websocket_client_error(event_loop):
    client = _make_client(event_loop)

    inner = MagicMock()
    inner.send_pong = AsyncMock(side_effect=WebSocketClientError("boom"))
    client._clients[0] = inner

    event_loop.run_until_complete(client.send_pong(0, b"ping-payload"))

    inner.send_pong.assert_awaited_once_with(b"ping-payload")


def test_send_pong_returns_when_client_missing(event_loop):
    client = _make_client(event_loop)
    client._clients[0] = None

    # Early return when no inner client; must not raise
    event_loop.run_until_complete(client.send_pong(0, b"ping-payload"))


def test_on_pong_task_done_skips_cancelled_task(event_loop):
    client = _make_client(event_loop)

    async def coro():
        await asyncio.sleep(1)

    task = event_loop.create_task(coro())
    task.cancel()
    with pytest.raises(asyncio.CancelledError):
        event_loop.run_until_complete(task)

    # Calling exception() on a cancelled task raises CancelledError; the
    # guard must short-circuit before that happens.
    client._on_pong_task_done(task)


def test_handle_ping_registers_done_callback_for_send_pong_task(event_loop):
    client = _make_client(event_loop)

    async def raising_send_pong(_client_id, _raw):
        raise ValueError("unexpected")

    client.send_pong = raising_send_pong

    captured: list[asyncio.Task] = []
    original = client._on_pong_task_done

    def spy(task: asyncio.Task) -> None:
        captured.append(task)
        original(task)  # exercise the production callback

    # Patch before _handle_ping runs so add_done_callback captures the spy
    client._on_pong_task_done = spy

    # Replace the Nautilus logger with a mock; the production callback emits
    # a warning whenever it retrieves a non-None exception, so observing the
    # warning transitively proves task.exception() was called.
    log_mock = MagicMock()
    client._log = log_mock

    client._handle_ping(0, b"ping-payload")

    pending = [t for t in asyncio.all_tasks(event_loop) if not t.done()]
    event_loop.run_until_complete(asyncio.wait(pending))

    # If _handle_ping stops calling add_done_callback, captured stays empty
    # and this assertion fails: the regression the test is named for.
    assert len(captured) == 1
    # If _on_pong_task_done stops calling task.exception(), no warning fires
    log_mock.warning.assert_called_once()
    msg = log_mock.warning.call_args.args[0]
    assert "Unhandled exception in send_pong task" in msg
    assert "ValueError" in msg
