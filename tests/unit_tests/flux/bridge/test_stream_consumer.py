from __future__ import annotations

import json
from typing import Any

import pytest

from nautilus_trader.flux.bridge.stream_consumer import FluxBridgeStreamConsumer
from nautilus_trader.flux.bridge.stream_consumer import build_parser


class _FakeRedis:
    pass


def _consumer() -> FluxBridgeStreamConsumer:
    return FluxBridgeStreamConsumer(
        redis_client=_FakeRedis(),
        environment="paper",
    )


def test_decode_entry_unwraps_flux_bus_payload_and_extracts_topic() -> None:
    consumer = _consumer()
    wrapped_payload = {
        "type": "nautilus_trader.flux.events.FluxBusPayload",
        "topic": "flux.strategy.trade",
        "payload": {"trade_id": "t-1", "ts_event": "1700000001"},
    }
    fields = {"payload": json.dumps(wrapped_payload)}

    decoded = consumer._decode_entry(
        stream_key="flux:v1:in:stream:paper:maker_v3_01:event",
        entry_id="1700000001000-0",
        fields=fields,
    )

    assert decoded is not None
    payload, context = decoded
    assert payload == {"trade_id": "t-1", "ts_event": "1700000001"}
    assert context.topic == "trade"
    assert context.ts_ms == 1700000001000


def test_decode_entry_uses_ts_event_fallback_for_ts_ms() -> None:
    consumer = _consumer()
    fields = {"payload": json.dumps({"event": "refresh", "ts_event": "1700000010"})}

    decoded = consumer._decode_entry(
        stream_key="flux:v1:in:stream:paper:maker_v3_01:event",
        entry_id="1700000001000-0",
        fields=fields,
    )

    assert decoded is not None
    _payload, context = decoded
    assert context.ts_ms == 1700000010000


def test_decode_entry_extracts_ts_ms_from_flux_bus_rows_payload() -> None:
    consumer = _consumer()
    wrapped_payload = {
        "type": "nautilus_trader.flux.events.FluxBusPayload",
        "topic": "flux.makerv3.fv",
        "payload": [
            {
                "fv": "0.0097",
                "ts_ms": 1700000022000,
            },
        ],
    }
    fields = {"payload": json.dumps(wrapped_payload)}

    decoded = consumer._decode_entry(
        stream_key="flux:v1:in:stream:paper:maker_v3_01:fv",
        entry_id="1700000001000-0",
        fields=fields,
    )

    assert decoded is not None
    payload, context = decoded
    assert payload == {"rows": [{"fv": "0.0097", "ts_ms": 1700000022000}]}
    assert context.topic == "fv"
    assert context.ts_ms == 1700000022000


def test_decode_entry_fails_fast_for_missing_parseable_timestamp() -> None:
    consumer = _consumer()
    fields = {"payload": "{this-is-not-json"}

    with pytest.raises(ValueError, match="timestamp"):
        consumer._decode_entry(
            stream_key="flux:v1:in:stream:paper:maker_v3_01:event",
            entry_id="1700000001000-0",
            fields=fields,
        )


def test_parser_defaults_environment_for_runner_ergonomics() -> None:
    parser = build_parser()

    args = parser.parse_args([])

    assert args.environment == "paper"


class _RunLoopRedis:
    def __init__(
        self,
        stream_key: str,
        entry_id: str,
        fields: dict[Any, Any],
        entries: list[tuple[str, dict[Any, Any]]] | None = None,
    ) -> None:
        self._stream_key = stream_key
        self._entries = entries or [(entry_id, fields)]

    def xread(
        self,
        *,
        streams: dict[str, str],
        count: int,
        block: int,
    ) -> list[tuple[bytes, list[tuple[bytes, dict[Any, Any]]]]]:
        _ = streams, count, block
        encoded_entries = [(entry_id.encode(), fields) for entry_id, fields in self._entries]
        return [
            (
                self._stream_key.encode(),
                encoded_entries,
            ),
        ]


class _LegacyDisconnectPool:
    def __init__(self) -> None:
        self.disconnect_calls: list[bool] = []

    def disconnect(self, inuse_connections: bool = True) -> None:
        self.disconnect_calls.append(inuse_connections)


class _ShutdownRedis(_RunLoopRedis):
    def __init__(self, stream_key: str, entry_id: str, fields: dict[Any, Any]) -> None:
        super().__init__(stream_key=stream_key, entry_id=entry_id, fields=fields)
        self.closed = False
        self.connection_pool = _LegacyDisconnectPool()

    def close(self) -> None:
        self.closed = True


def _build_run_consumer(
    *,
    handler,
    stream_key: str = "flux:v1:in:stream:paper:strategy_01:event",
    entry_id: str = "1700000001000-0",
    fields: dict[Any, Any] | None = None,
    entries: list[tuple[str, dict[Any, Any]]] | None = None,
) -> tuple[FluxBridgeStreamConsumer, str, str]:
    raw_fields = fields or {"payload": json.dumps({"event": "refresh", "ts_event": "1700000010"})}
    consumer = FluxBridgeStreamConsumer(
        redis_client=_RunLoopRedis(
            stream_key=stream_key,
            entry_id=entry_id,
            fields=raw_fields,
            entries=entries,
        ),
        environment="paper",
        handlers={"event": handler},
        topics=["event"],
    )
    consumer._stream_ids = {stream_key: "$"}
    consumer._install_signals = lambda: None
    consumer._refresh_streams = lambda *, force=False: None
    return consumer, stream_key, entry_id


def test_run_does_not_advance_stream_offset_on_decode_failure() -> None:
    def _handler(payload, context):
        _ = payload, context
        return []

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)

    def _decode_fail(*, stream_key: str, entry_id: str, fields: dict[Any, Any]):
        consumer._running = False
        raise ValueError("decode failed")

    consumer._decode_entry = _decode_fail
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"


def test_run_does_not_advance_stream_offset_on_handler_failure() -> None:
    def _handler(payload, context):
        _ = payload, context
        consumer._running = False
        raise RuntimeError("handler failed")

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"


def test_run_does_not_advance_stream_offset_on_write_failure() -> None:
    def _handler(payload, context):
        _ = payload, context
        return []

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)

    def _write_fail(_ops):
        consumer._running = False
        raise RuntimeError("write failed")

    consumer._apply_write_ops = _write_fail
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"


def test_run_advances_stream_offset_after_successful_write() -> None:
    def _handler(payload, context):
        _ = payload, context
        return []

    consumer, stream_key, entry_id = _build_run_consumer(handler=_handler)

    def _write_ok(_ops):
        consumer._running = False

    consumer._apply_write_ops = _write_ok
    consumer.run()

    assert consumer._stream_ids[stream_key] == entry_id


def test_run_stops_processing_stream_batch_after_first_decode_failure() -> None:
    handled_entry_ids: list[str] = []

    def _handler(payload, context):
        _ = payload
        handled_entry_ids.append(context.entry_id)
        return []

    first_entry_id = "1700000001000-0"
    second_entry_id = "1700000001001-0"
    first_fields = {"payload": json.dumps({"event": "refresh", "ts_event": "1700000010"})}
    second_fields = {"payload": json.dumps({"event": "refresh", "ts_event": "1700000011"})}
    consumer, stream_key, _entry_id = _build_run_consumer(
        handler=_handler,
        entry_id=first_entry_id,
        fields=first_fields,
        entries=[(first_entry_id, first_fields), (second_entry_id, second_fields)],
    )
    decode_entry = consumer._decode_entry

    def _decode_fail_first(*, stream_key: str, entry_id: str, fields: dict[Any, Any]):
        if entry_id == first_entry_id:
            consumer._running = False
            raise ValueError("decode failed")
        return decode_entry(stream_key=stream_key, entry_id=entry_id, fields=fields)

    consumer._decode_entry = _decode_fail_first
    consumer.run()

    assert handled_entry_ids == []
    assert consumer._stream_ids[stream_key] == "$"


def test_run_closes_redis_on_exit_with_legacy_disconnect_pool() -> None:
    stream_key = "flux:v1:in:stream:paper:strategy_01:event"
    entry_id = "1700000001000-0"
    fields = {"payload": json.dumps({"event": "refresh", "ts_event": "1700000010"})}
    redis_client = _ShutdownRedis(stream_key=stream_key, entry_id=entry_id, fields=fields)

    def _handler(payload, context):
        _ = payload, context
        return []

    consumer = FluxBridgeStreamConsumer(
        redis_client=redis_client,
        environment="paper",
        handlers={"event": _handler},
        topics=["event"],
    )
    consumer._stream_ids = {stream_key: "$"}
    consumer._install_signals = lambda: None
    consumer._refresh_streams = lambda *, force=False: None

    def _write_ok(_ops):
        consumer._running = False

    consumer._apply_write_ops = _write_ok
    consumer.run()

    assert redis_client.closed is True
    assert redis_client.connection_pool.disconnect_calls == [False]
