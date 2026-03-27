from __future__ import annotations

import json
from typing import Any

import pytest

from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.bridge.stream_consumer import FluxBridgeStreamConsumer
from nautilus_trader.flux.bridge.stream_consumer import build_parser


class _FakeRedis:
    pass


def _consumer() -> FluxBridgeStreamConsumer:
    return FluxBridgeStreamConsumer(
        redis_client=_FakeRedis(),
        environment="paper",
    )


class _TrackStreamRedis:
    def __init__(
        self,
        *,
        stream_types: dict[str, str] | None = None,
        stream_sizes: dict[str, int] | None = None,
        latest_entries: dict[str, str] | None = None,
    ) -> None:
        self._stream_types = dict(stream_types or {})
        self._stream_sizes = dict(stream_sizes or {})
        self._latest_entries = dict(latest_entries or {})

    def type(self, key: str) -> str:
        return self._stream_types.get(key, "none")

    def xlen(self, key: str) -> int:
        return int(self._stream_sizes.get(key, 0))

    def xrevrange(self, key: str, _max: str, _min: str, *, count: int = 1):
        latest_entry = self._latest_entries.get(key)
        if latest_entry is None or count <= 0:
            return []
        return [(latest_entry.encode(), {b"payload": b"{}"})]


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


def test_track_stream_key_backfills_trade_stream_when_materialized_stream_empty() -> None:
    stream_key = "flux:v1:in:stream:live:msft_tradexyz_makerv4:flux.makerv3.trade"
    target_stream_key = "flux:v1:trades:stream:msft_tradexyz_makerv4"
    consumer = FluxBridgeStreamConsumer(
        redis_client=_TrackStreamRedis(
            stream_types={stream_key: "stream"},
            stream_sizes={target_stream_key: 0},
        ),
        environment="live",
        handlers={"flux.makerv3.trade": lambda payload, context: []},
        topics=["flux.makerv3.trade"],
    )

    consumer._track_stream_key(stream_key)

    assert consumer._stream_ids[stream_key] == "0-0"


def test_track_stream_key_pins_latest_source_entry_when_trade_stream_already_materialized() -> None:
    stream_key = "flux:v1:in:stream:live:nvda_tradexyz_makerv4:flux.makerv3.trade"
    target_stream_key = "flux:v1:trades:stream:nvda_tradexyz_makerv4"
    consumer = FluxBridgeStreamConsumer(
        redis_client=_TrackStreamRedis(
            stream_types={stream_key: "stream"},
            stream_sizes={target_stream_key: 4},
            latest_entries={stream_key: "1700000001000-9"},
        ),
        environment="live",
        handlers={"flux.makerv3.trade": lambda payload, context: []},
        topics=["flux.makerv3.trade"],
    )

    consumer._track_stream_key(stream_key)

    assert consumer._stream_ids[stream_key] == "1700000001000-9"


def test_track_stream_key_pins_latest_source_entry_for_non_trade_topics() -> None:
    stream_key = "flux:v1:in:stream:live:nvda_tradexyz_makerv4:flux.makerv3.state"
    consumer = FluxBridgeStreamConsumer(
        redis_client=_TrackStreamRedis(
            stream_types={stream_key: "stream"},
            latest_entries={stream_key: "1700000001001-4"},
        ),
        environment="live",
        handlers={"flux.makerv3.state": lambda payload, context: []},
        topics=["flux.makerv3.state"],
    )

    consumer._track_stream_key(stream_key)

    assert consumer._stream_ids[stream_key] == "1700000001001-4"


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


def test_decode_entry_routes_grouped_streams_to_payload_strategy_id() -> None:
    consumer = FluxBridgeStreamConsumer(
        redis_client=_FakeRedis(),
        environment="live",
        strategy_ids=["aapl_tradexyz_maker", "aapl_tradexyz_taker"],
        stream_strategy_ids=["aapl_tradexyz"],
    )
    fields = {
        "payload": json.dumps(
            {
                "strategy_id": "aapl_tradexyz_taker",
                "ready": True,
                "ts_ms": 1700000022000,
            },
        ),
    }

    decoded = consumer._decode_entry(
        stream_key="flux:v1:in:stream:live:aapl_tradexyz:state",
        entry_id="1700000001000-0",
        fields=fields,
    )

    assert decoded is not None
    payload, context = decoded
    assert payload["strategy_id"] == "aapl_tradexyz_taker"
    assert context.strategy_id == "aapl_tradexyz_taker"
    assert context.topic == "state"


def test_decode_entry_preserves_stream_key_strategy_for_non_grouped_streams() -> None:
    consumer = FluxBridgeStreamConsumer(
        redis_client=_FakeRedis(),
        environment="paper",
        strategy_ids=["strategy_01"],
        stream_strategy_ids=["strategy_01"],
    )
    fields = {
        "payload": json.dumps(
            {
                "strategy_id": "strategy_02",
                "ts_ms": 1700000022000,
            },
        ),
    }

    decoded = consumer._decode_entry(
        stream_key="flux:v1:in:stream:paper:strategy_01:event",
        entry_id="1700000001000-0",
        fields=fields,
    )

    assert decoded is not None
    _payload, context = decoded
    assert context.strategy_id == "strategy_01"


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
        self.streams: dict[str, list[tuple[str, dict[Any, Any]]]] = {}
        self._xadd_counter = 0

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

    def xadd(
        self,
        key: str,
        fields: dict[Any, Any],
        *,
        maxlen: int | None = None,
        approximate: bool = True,
    ) -> str:
        _ = maxlen, approximate
        self._xadd_counter += 1
        entry_id = f"{self._xadd_counter}-0"
        self.streams.setdefault(key, []).append((entry_id, dict(fields)))
        return entry_id


class _MultiStreamRunLoopRedis:
    def __init__(
        self,
        streams: list[tuple[str, list[tuple[str, dict[Any, Any]]]]],
    ) -> None:
        self._streams = list(streams)
        self.streams: dict[str, list[tuple[str, dict[Any, Any]]]] = {}
        self._xadd_counter = 0

    def xread(
        self,
        *,
        streams: dict[str, str],
        count: int,
        block: int,
    ) -> list[tuple[bytes, list[tuple[bytes, dict[Any, Any]]]]]:
        _ = streams, count, block
        return [
            (
                stream_key.encode(),
                [(entry_id.encode(), fields) for entry_id, fields in entries],
            )
            for stream_key, entries in self._streams
        ]

    def xadd(
        self,
        key: str,
        fields: dict[Any, Any],
        *,
        maxlen: int | None = None,
        approximate: bool = True,
    ) -> str:
        _ = maxlen, approximate
        self._xadd_counter += 1
        entry_id = f"{self._xadd_counter}-0"
        self.streams.setdefault(key, []).append((entry_id, dict(fields)))
        return entry_id


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
    alert_key = FluxRedisKeys(strategy_id="strategy_01").alerts()
    assert len(consumer._redis.streams[alert_key]) == 1
    alert_payload = json.loads(consumer._redis.streams[alert_key][0][1]["payload"])
    assert alert_payload["alert_key"] == "bridge_handler_failed"
    assert alert_payload["strategy_id"] == "strategy_01"
    assert alert_payload["source"] == "bridge"
    assert alert_payload["actionable"] is True
    assert alert_payload["source_topic"] == "event"


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
    alert_key = FluxRedisKeys(strategy_id="strategy_01").alerts()
    assert len(consumer._redis.streams[alert_key]) == 1
    alert_payload = json.loads(consumer._redis.streams[alert_key][0][1]["payload"])
    assert alert_payload["alert_key"] == "bridge_write_failed"
    assert alert_payload["strategy_id"] == "strategy_01"
    assert alert_payload["source"] == "bridge"
    assert alert_payload["source_topic"] == "event"


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


def test_run_skips_grouped_execution_alert_decode_failure_and_continues_other_streams() -> None:
    bad_alert_stream = "flux:v1:in:stream:live:nvda_tradexyz:flux.execution.alert"
    good_state_stream = "flux:v1:in:stream:live:orcl_tradexyz:flux.makerv3.state"
    bad_alert_entry_id = "1700000001000-0"
    good_state_entry_id = "1700000001001-0"
    bad_alert_fields = {
        "payload": json.dumps(
            {
                "type": "FluxBusPayload",
                "topic": "flux.execution.alert",
                "payload": json.dumps(
                    {
                        "message": "grouped alert without external strategy id",
                        "ts_ms": 1700000001000,
                    },
                ),
                "ts_event": "0",
                "ts_init": "0",
            },
        ),
    }
    handled_state_strategy_ids: list[str] = []
    good_state_fields = {
        "payload": json.dumps(
            {
                "type": "FluxBusPayload",
                "topic": "flux.makerv3.state",
                "payload": json.dumps(
                    {
                        "strategy_id": "orcl_tradexyz_taker",
                        "state": "bot_off",
                        "ts_ms": 1700000001001,
                    },
                ),
                "ts_event": "0",
                "ts_init": "0",
            },
        ),
    }

    def _state_handler(payload, context):
        _ = payload
        handled_state_strategy_ids.append(context.strategy_id)
        consumer._running = False
        return []

    consumer = FluxBridgeStreamConsumer(
        redis_client=_MultiStreamRunLoopRedis(
            [
                (bad_alert_stream, [(bad_alert_entry_id, bad_alert_fields)]),
                (good_state_stream, [(good_state_entry_id, good_state_fields)]),
            ],
        ),
        environment="live",
        strategy_ids=[
            "nvda_tradexyz_maker",
            "nvda_tradexyz_taker",
            "orcl_tradexyz_maker",
            "orcl_tradexyz_taker",
        ],
        stream_strategy_ids=["nvda_tradexyz", "orcl_tradexyz"],
        handlers={
            "flux.execution.alert": lambda payload, context: [],
            "flux.makerv3.state": _state_handler,
        },
        topics=["flux.execution.alert", "flux.makerv3.state"],
    )
    consumer._stream_ids = {
        bad_alert_stream: "$",
        good_state_stream: "$",
    }
    consumer._install_signals = lambda: None
    consumer._refresh_streams = lambda *, force=False: None
    original_xread = consumer._redis.xread
    xread_calls = 0

    def _xread_once_then_stop(*, streams, count, block):
        nonlocal xread_calls
        xread_calls += 1
        if xread_calls > 1:
            consumer._running = False
            return []
        return original_xread(streams=streams, count=count, block=block)

    consumer._redis.xread = _xread_once_then_stop

    consumer.run()

    assert consumer._stream_ids[bad_alert_stream] == bad_alert_entry_id
    assert consumer._stream_ids[good_state_stream] == good_state_entry_id
    assert handled_state_strategy_ids == ["orcl_tradexyz_taker"]
    alert_key = FluxRedisKeys(strategy_id="nvda_tradexyz").alerts()
    assert len(consumer._redis.streams[alert_key]) == 1
    alert_payload = json.loads(consumer._redis.streams[alert_key][0][1]["payload"])
    assert alert_payload["alert_key"] == "bridge_decode_failed"
    assert alert_payload["strategy_id"] == "nvda_tradexyz"
    assert alert_payload["source_topic"] == "decode"


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
