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
        redis_client=_FakeRedis(),  # type: ignore[arg-type]
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

    decoded = consumer._decode_entry(  # noqa: SLF001
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

    decoded = consumer._decode_entry(  # noqa: SLF001
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

    decoded = consumer._decode_entry(  # noqa: SLF001
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
        consumer._decode_entry(  # noqa: SLF001
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

    def xread(self, *, streams: dict[str, str], count: int, block: int) -> list[tuple[bytes, list[tuple[bytes, dict[Any, Any]]]]]:
        _ = streams, count, block
        encoded_entries = [
            (entry_id.encode(), fields)
            for entry_id, fields in self._entries
        ]
        return [
            (
                self._stream_key.encode(),
                encoded_entries,
            ),
        ]


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
        redis_client=_RunLoopRedis(  # type: ignore[arg-type]
            stream_key=stream_key,
            entry_id=entry_id,
            fields=raw_fields,
            entries=entries,
        ),
        environment="paper",
        handlers={"event": handler},
        topics=["event"],
    )
    consumer._stream_ids = {stream_key: "$"}  # noqa: SLF001
    consumer._install_signals = lambda: None  # noqa: SLF001
    consumer._refresh_streams = lambda *, force=False: None  # noqa: SLF001
    return consumer, stream_key, entry_id


def test_run_does_not_advance_stream_offset_on_decode_failure() -> None:
    def _handler(payload, context):  # noqa: ANN001, ANN202
        _ = payload, context
        return []

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)

    def _decode_fail(*, stream_key: str, entry_id: str, fields: dict[Any, Any]):  # noqa: ARG001
        consumer._running = False  # noqa: SLF001
        raise ValueError("decode failed")

    consumer._decode_entry = _decode_fail  # type: ignore[method-assign]  # noqa: SLF001
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"  # noqa: SLF001


def test_run_does_not_advance_stream_offset_on_handler_failure() -> None:
    def _handler(payload, context):  # noqa: ANN001, ANN202
        _ = payload, context
        consumer._running = False  # noqa: SLF001
        raise RuntimeError("handler failed")

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"  # noqa: SLF001


def test_run_does_not_advance_stream_offset_on_write_failure() -> None:
    def _handler(payload, context):  # noqa: ANN001, ANN202
        _ = payload, context
        return []

    consumer, stream_key, _entry_id = _build_run_consumer(handler=_handler)

    def _write_fail(_ops):  # noqa: ANN001, ANN202
        consumer._running = False  # noqa: SLF001
        raise RuntimeError("write failed")

    consumer._apply_write_ops = _write_fail  # type: ignore[method-assign]  # noqa: SLF001
    consumer.run()

    assert consumer._stream_ids[stream_key] == "$"  # noqa: SLF001


def test_run_advances_stream_offset_after_successful_write() -> None:
    def _handler(payload, context):  # noqa: ANN001, ANN202
        _ = payload, context
        return []

    consumer, stream_key, entry_id = _build_run_consumer(handler=_handler)

    def _write_ok(_ops):  # noqa: ANN001, ANN202
        consumer._running = False  # noqa: SLF001

    consumer._apply_write_ops = _write_ok  # type: ignore[method-assign]  # noqa: SLF001
    consumer.run()

    assert consumer._stream_ids[stream_key] == entry_id  # noqa: SLF001


def test_run_stops_processing_stream_batch_after_first_decode_failure() -> None:
    handled_entry_ids: list[str] = []

    def _handler(payload, context):  # noqa: ANN001, ANN202
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
    decode_entry = consumer._decode_entry  # noqa: SLF001

    def _decode_fail_first(*, stream_key: str, entry_id: str, fields: dict[Any, Any]):  # noqa: ARG001
        if entry_id == first_entry_id:
            consumer._running = False  # noqa: SLF001
            raise ValueError("decode failed")
        return decode_entry(stream_key=stream_key, entry_id=entry_id, fields=fields)

    consumer._decode_entry = _decode_fail_first  # type: ignore[method-assign]  # noqa: SLF001
    consumer.run()

    assert handled_entry_ids == []
    assert consumer._stream_ids[stream_key] == "$"  # noqa: SLF001
