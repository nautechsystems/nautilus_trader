import asyncio
from types import SimpleNamespace
from unittest.mock import AsyncMock

import pytest

from nautilus_trader.adapters.binance.data import BinanceCommonDataClient
from nautilus_trader.adapters.binance.websocket.client import BinanceWebSocketClient
from nautilus_trader.common.component import LiveClock


def _build_ws_client(loop: asyncio.AbstractEventLoop) -> BinanceWebSocketClient:
    return BinanceWebSocketClient(
        clock=LiveClock(),
        base_url="wss://stream.binance.com:9443",
        handler=lambda _raw: None,
        handler_reconnect=None,
        loop=loop,
    )


@pytest.mark.asyncio
async def test_book_ticker_recovery_replays_desired_stream_on_healthy_transport() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    ws_client = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
        send_text=AsyncMock(),
    )
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = ws_client
    client._client_streams[0] = [stream]

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is True
    assert result["action"] == "replay"
    assert result["stream"] == stream
    assert result["client_id"] == 0
    assert client.recovery_snapshot(stream)["desired"] is True
    ws_client.send_text.assert_awaited_once()


@pytest.mark.asyncio
async def test_book_ticker_recovery_collapses_duplicate_requests() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]

    started = asyncio.Event()
    release = asyncio.Event()
    calls = 0

    async def slow_send(_client_id: int, _msg: dict[str, object]) -> bool:
        nonlocal calls
        calls += 1
        started.set()
        await release.wait()
        return True

    client._send = AsyncMock(side_effect=slow_send)

    first = asyncio.create_task(client.recover_book_ticker("BTCUSDT"))
    await started.wait()
    assert client.recovery_snapshot(stream)["in_flight"] is True
    second = asyncio.create_task(client.recover_book_ticker("BTCUSDT"))
    release.set()

    results = await asyncio.gather(first, second)

    assert calls == 1
    assert results[0]["action"] == "replay"
    assert results[1]["action"] == "replay"
    assert client.recovery_snapshot(stream)["desired"] is True
    assert client.recovery_snapshot(stream)["in_flight"] is False


@pytest.mark.asyncio
async def test_book_ticker_recovery_reuses_completed_success_for_same_version() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]
    client._send = AsyncMock(return_value=True)

    first = await client.recover_book_ticker("BTCUSDT")
    second = await client.recover_book_ticker("BTCUSDT")

    assert first["action"] == "replay"
    assert second == first
    assert client._send.await_count == 1


@pytest.mark.asyncio
async def test_book_ticker_recovery_retries_again_after_completed_failure() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]
    client._send = AsyncMock(return_value=False)
    client._disconnect_client = AsyncMock(side_effect=RuntimeError("disconnect boom"))
    client._connect_client = AsyncMock()

    first = await client.recover_book_ticker("BTCUSDT")
    second = await client.recover_book_ticker("BTCUSDT")

    assert first["action"] == "reconnect_failed"
    assert second["action"] == "reconnect_failed"
    assert client._disconnect_client.await_count == 2


@pytest.mark.asyncio
async def test_book_ticker_recovery_reconnects_after_replay_failure() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]
    client._send = AsyncMock(return_value=False)

    async def disconnect_side_effect(_client_id: int) -> None:
        client._bump_stream_version(stream)

    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock()

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is True
    assert result["action"] == "reconnect"
    assert result["stream"] == stream
    assert result["client_id"] == 0
    client._connect_client.assert_awaited_once_with(0, [stream])
    assert client.recovery_snapshot(stream)["desired"] is True


@pytest.mark.asyncio
async def test_book_ticker_recovery_reconnects_when_version_changes_but_stream_stays_desired() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]

    async def send_side_effect(_client_id: int, _msg: dict[str, object]) -> bool:
        client._bump_stream_version(stream)
        return True

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock()
    client._connect_client = AsyncMock()

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is True
    assert result["action"] == "reconnect"
    assert result["stream"] == stream
    client._disconnect_client.assert_awaited_once_with(0)
    client._connect_client.assert_awaited_once_with(0, [stream])


@pytest.mark.asyncio
async def test_book_ticker_recovery_dedupes_shared_client_reconnects_for_sibling_streams() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    btc_stream = "btcusdt@bookTicker"
    eth_stream = "ethusdt@bookTicker"
    client._desired_streams.update({btc_stream, eth_stream})
    client._streams.extend([btc_stream, eth_stream])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [btc_stream, eth_stream]

    release_send = asyncio.Event()
    both_replays_started = asyncio.Event()
    replay_attempts = 0

    async def send_side_effect(_client_id: int, msg: dict[str, object]) -> bool:
        nonlocal replay_attempts
        stream = msg["params"][0]
        client._bump_stream_version(stream)
        replay_attempts += 1
        if replay_attempts == 2:
            both_replays_started.set()
        await release_send.wait()
        return True

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock()
    client._connect_client = AsyncMock()

    btc_task = asyncio.create_task(client.recover_book_ticker("BTCUSDT"))
    eth_task = asyncio.create_task(client.recover_book_ticker("ETHUSDT"))
    await both_replays_started.wait()
    release_send.set()

    btc_result, eth_result = await asyncio.gather(btc_task, eth_task)

    assert btc_result["action"] == "reconnect"
    assert eth_result["action"] == "reconnect"
    client._disconnect_client.assert_awaited_once_with(0)
    client._connect_client.assert_awaited_once_with(0, [btc_stream, eth_stream])


@pytest.mark.asyncio
async def test_book_ticker_recovery_reuses_completed_client_reconnect_for_late_sibling() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    btc_stream = "btcusdt@bookTicker"
    eth_stream = "ethusdt@bookTicker"
    client._desired_streams.update({btc_stream, eth_stream})
    client._streams.extend([btc_stream, eth_stream])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [btc_stream, eth_stream]

    eth_replay_started = asyncio.Event()
    reconnect_completed = asyncio.Event()

    async def send_side_effect(_client_id: int, msg: dict[str, object]) -> bool:
        stream = msg["params"][0]
        if stream == eth_stream:
            eth_replay_started.set()
            await reconnect_completed.wait()
            while client._client_reconnect_tasks.get(0) is not None:
                await asyncio.sleep(0)
            return True

        assert stream == btc_stream
        client._bump_stream_version(btc_stream)
        return True

    async def disconnect_side_effect(client_id: int) -> None:
        for stream in client._client_streams.get(client_id, []):
            client._bump_stream_version(stream)

    async def connect_side_effect(_client_id: int, _streams: list[str]) -> None:
        reconnect_completed.set()

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock(side_effect=connect_side_effect)

    eth_task = asyncio.create_task(client.recover_book_ticker("ETHUSDT"))
    await eth_replay_started.wait()
    btc_task = asyncio.create_task(client.recover_book_ticker("BTCUSDT"))

    eth_result, btc_result = await asyncio.gather(eth_task, btc_task)

    assert btc_result["action"] == "reconnect"
    assert eth_result["action"] == "reconnect"
    client._disconnect_client.assert_awaited_once_with(0)
    client._connect_client.assert_awaited_once_with(0, [btc_stream, eth_stream])


@pytest.mark.asyncio
async def test_book_ticker_recovery_marks_shared_reconnect_failed_when_sibling_subscribe_fails(
    monkeypatch: pytest.MonkeyPatch,
) -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    btc_stream = "btcusdt@bookTicker"
    eth_stream = "ethusdt@bookTicker"
    client._desired_streams.update({btc_stream, eth_stream})
    client._streams.extend([btc_stream, eth_stream])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [btc_stream, eth_stream]

    connect_mock = AsyncMock(
        return_value=SimpleNamespace(
            is_disconnecting=lambda: False,
            is_closed=lambda: False,
            send_text=AsyncMock(),
        ),
    )
    monkeypatch.setattr(
        "nautilus_trader.adapters.binance.websocket.client.WebSocketClient.connect",
        connect_mock,
    )

    async def send_side_effect(_client_id: int, msg: dict[str, object]) -> bool:
        stream = msg["params"][0]
        if stream == btc_stream:
            return False

        assert msg["params"] == [eth_stream]
        return False

    async def disconnect_side_effect(client_id: int) -> None:
        for stream in client._client_streams.get(client_id, []):
            client._bump_stream_version(stream)

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)

    btc_result = await client.recover_book_ticker("BTCUSDT")
    eth_result = await client.recover_book_ticker("ETHUSDT")

    assert btc_result["ok"] is False
    assert btc_result["action"] == "reconnect_failed"
    assert eth_result["ok"] is False
    assert eth_result["action"] == "reconnect_failed"
    assert client._client_recovery_results[0]["ok"] is False


@pytest.mark.asyncio
async def test_book_ticker_recovery_does_not_reuse_stale_success_after_later_failed_reconnect() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    btc_stream = "btcusdt@bookTicker"
    eth_stream = "ethusdt@bookTicker"
    client._desired_streams.update({btc_stream, eth_stream})
    client._streams.extend([btc_stream, eth_stream])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [btc_stream, eth_stream]

    eth_replay_started = asyncio.Event()
    release_eth_replay = asyncio.Event()
    btc_send_calls = 0
    connect_attempts = 0

    async def send_side_effect(_client_id: int, msg: dict[str, object]) -> bool:
        nonlocal btc_send_calls
        stream = msg["params"][0]
        if stream == eth_stream:
            eth_replay_started.set()
            await release_eth_replay.wait()
            return True

        assert stream == btc_stream
        btc_send_calls += 1
        if btc_send_calls == 1:
            client._bump_stream_version(btc_stream)
            return True

        return False

    async def disconnect_side_effect(client_id: int) -> None:
        for stream in client._client_streams.get(client_id, []):
            client._bump_stream_version(stream)

    async def connect_side_effect(_client_id: int, _streams: list[str]) -> None:
        nonlocal connect_attempts
        connect_attempts += 1
        if connect_attempts > 1:
            raise RuntimeError("reconnect boom")

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock(side_effect=connect_side_effect)

    eth_task = asyncio.create_task(client.recover_book_ticker("ETHUSDT"))
    await eth_replay_started.wait()

    first_btc_result = await client.recover_book_ticker("BTCUSDT")
    client._bump_stream_version(btc_stream)
    failed_btc_result = await client.recover_book_ticker("BTCUSDT")

    release_eth_replay.set()
    eth_result = await eth_task

    assert first_btc_result["action"] == "reconnect"
    assert failed_btc_result["action"] == "reconnect_failed"
    assert eth_result["ok"] is False
    assert eth_result["action"] == "reconnect_failed"
    assert "reconnect boom" in eth_result["error"]
    assert client._disconnect_client.await_count == 2
    assert client._connect_client.await_count == 2


@pytest.mark.asyncio
async def test_book_ticker_recovery_waits_for_newer_in_flight_reconnect_before_reusing_client_state() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    btc_stream = "btcusdt@bookTicker"
    eth_stream = "ethusdt@bookTicker"
    client._desired_streams.update({btc_stream, eth_stream})
    client._streams.extend([btc_stream, eth_stream])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [btc_stream, eth_stream]

    eth_replay_started = asyncio.Event()
    release_eth_replay = asyncio.Event()
    second_reconnect_started = asyncio.Event()
    release_second_reconnect = asyncio.Event()
    connect_attempts = 0

    async def send_side_effect(_client_id: int, msg: dict[str, object]) -> bool:
        stream = msg["params"][0]
        if stream == eth_stream:
            eth_replay_started.set()
            await release_eth_replay.wait()
            return True

        assert stream == btc_stream
        return False

    async def disconnect_side_effect(client_id: int) -> None:
        for stream in client._client_streams.get(client_id, []):
            client._bump_stream_version(stream)

    async def connect_side_effect(_client_id: int, _streams: list[str]) -> None:
        nonlocal connect_attempts
        connect_attempts += 1
        if connect_attempts == 2:
            second_reconnect_started.set()
            await release_second_reconnect.wait()
            raise RuntimeError("reconnect boom")

    client._send = AsyncMock(side_effect=send_side_effect)
    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock(side_effect=connect_side_effect)

    eth_task = asyncio.create_task(client.recover_book_ticker("ETHUSDT"))
    await eth_replay_started.wait()

    first_btc_result = await client.recover_book_ticker("BTCUSDT")
    client._bump_stream_version(btc_stream)
    second_btc_task = asyncio.create_task(client.recover_book_ticker("BTCUSDT"))
    await second_reconnect_started.wait()

    release_eth_replay.set()
    for _ in range(5):
        await asyncio.sleep(0)

    assert eth_task.done() is False

    release_second_reconnect.set()
    second_btc_result = await second_btc_task
    eth_result = await eth_task

    assert first_btc_result["action"] == "reconnect"
    assert second_btc_result["action"] == "reconnect_failed"
    assert eth_result["ok"] is False
    assert eth_result["action"] == "reconnect_failed"
    assert "reconnect boom" in eth_result["error"]
    assert client._disconnect_client.await_count == 2
    assert client._connect_client.await_count == 2


@pytest.mark.asyncio
async def test_book_ticker_recovery_reports_explicit_failure_when_reconnect_fails() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]
    client._send = AsyncMock(return_value=False)
    client._disconnect_client = AsyncMock()
    client._connect_client = AsyncMock(side_effect=RuntimeError("reconnect boom"))

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is False
    assert result["action"] == "reconnect_failed"
    assert result["stream"] == stream
    assert "reconnect boom" in result["error"]
    assert client.recovery_snapshot(stream)["desired"] is True


@pytest.mark.asyncio
async def test_book_ticker_recovery_respects_desired_state_changes_during_reconnect_window() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    client._desired_streams.add(stream)
    client._streams.append(stream)
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream]
    client._send = AsyncMock(return_value=False)

    async def disconnect_side_effect(client_id: int) -> None:
        client._desired_streams.discard(stream)
        if stream in client._streams:
            client._streams.remove(stream)
        if stream in client._client_streams.get(client_id, []):
            client._client_streams[client_id].remove(stream)
        client._bump_stream_version(stream)

    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock()

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is False
    assert result["action"] == "not_desired"
    assert result["stream"] == stream
    assert stream not in client._desired_streams
    assert stream not in client._streams
    assert stream not in client._client_streams.get(0, [])
    client._connect_client.assert_not_awaited()


@pytest.mark.asyncio
async def test_book_ticker_recovery_reconnects_other_desired_streams_when_target_becomes_undesired() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    stream = "btcusdt@bookTicker"
    sibling = "ethusdt@bookTicker"
    client._desired_streams.update({stream, sibling})
    client._streams.extend([stream, sibling])
    client._clients[0] = SimpleNamespace(
        is_disconnecting=lambda: False,
        is_closed=lambda: False,
    )
    client._client_streams[0] = [stream, sibling]
    client._send = AsyncMock(return_value=False)

    async def disconnect_side_effect(client_id: int) -> None:
        client._desired_streams.discard(stream)
        if stream in client._streams:
            client._streams.remove(stream)
        if stream in client._client_streams.get(client_id, []):
            client._client_streams[client_id].remove(stream)
        client._bump_stream_version(stream)

    client._disconnect_client = AsyncMock(side_effect=disconnect_side_effect)
    client._connect_client = AsyncMock()

    result = await client.recover_book_ticker("BTCUSDT")

    assert result["ok"] is False
    assert result["action"] == "not_desired"
    assert client._client_streams[0] == [sibling]
    client._connect_client.assert_awaited_once_with(0, [sibling])


@pytest.mark.asyncio
async def test_handle_reconnect_resubscribes_only_desired_streams() -> None:
    client = _build_ws_client(asyncio.get_running_loop())
    client._desired_streams.add("btcusdt@bookTicker")
    client._client_streams[0] = ["btcusdt@bookTicker", "ethusdt@bookTicker"]
    client._resubscribe_client = AsyncMock()

    client._handle_reconnect(0)
    await asyncio.sleep(0)

    client._resubscribe_client.assert_awaited_once_with(0, ["btcusdt@bookTicker"])
    assert client._client_streams[0] == ["btcusdt@bookTicker"]


@pytest.mark.asyncio
async def test_data_client_recover_quote_ticks_surfaces_websocket_outcome() -> None:
    instrument_id = SimpleNamespace(symbol=SimpleNamespace(value="BTCUSDT"))
    dummy = SimpleNamespace(
        _ws_client=SimpleNamespace(
            recover_book_ticker=AsyncMock(
                return_value={"ok": True, "action": "replay", "stream": "btcusdt@bookTicker"},
            ),
        ),
    )

    result = await BinanceCommonDataClient.recover_quote_ticks(dummy, instrument_id)  # type: ignore[arg-type]

    assert result["ok"] is True
    dummy._ws_client.recover_book_ticker.assert_awaited_once_with("BTCUSDT")
