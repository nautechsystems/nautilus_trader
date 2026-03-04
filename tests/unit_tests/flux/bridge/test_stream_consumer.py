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
