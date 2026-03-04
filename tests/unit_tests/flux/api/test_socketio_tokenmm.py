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

from copy import deepcopy

import pytest

from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.socketio import apply_signal_delta_patch
from nautilus_trader.flux.api.socketio import build_signal_delta_patch
from nautilus_trader.flux.api.socketio import normalize_profile
from nautilus_trader.flux.api.socketio import profile_room
from nautilus_trader.flux.common.keys import FluxRedisKeys


SOCKET_EVENT_NAMES = {"market_update", "signal_delta", "trade_update"}


def _seed_required_schema_keys(redis_client, flux_config) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(keys.state(), {"bot_on": True, "managed_orders": 2, "ts_ms": 1700000000000})
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {
            "qty": "1.0",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": flux_config.identity.strategy_id, "fv": 100.0}])


def _seed_socket_rows(redis_client, flux_config, contract_catalog) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    for contract in contract_catalog:
        base, quote = contract.symbol.split("/", maxsplit=1)
        redis_client.set_json(
            keys.market_last(exchange=contract.exchange, base=base, quote=quote),
            {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000100},
        )

    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-001",
                "seq": 1,
                "version": 1,
                "ts_ms": 1700000000200,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "BUY",
                "price": 100.0,
                "qty": 1.5,
            },
        ],
    )
    redis_client.add_stream_rows(
        keys.alerts(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "alert-001",
                "ts_ms": 1700000000300,
                "message": "sample-alert",
            },
        ],
    )


def _room_size(socket_server, room: str) -> int:
    namespace_rooms = socket_server.manager.rooms.get("/", {})
    members = namespace_rooms.get(room)
    if members is None:
        return 0
    return len(members)


def _take_socket_packets(client) -> list[dict]:
    return [packet for packet in client.get_received() if packet.get("name") in SOCKET_EVENT_NAMES]


@pytest.mark.parametrize(
    ("raw", "expected"),
    [
        ("tokenm", "tokenmm"),
        ("TOKENMM", "tokenmm"),
        (" tokenm ", "tokenmm"),
        ("other", "other"),
        ("", ""),
    ],
)
def test_normalize_profile_maps_token_aliases(raw: str, expected: str) -> None:
    assert normalize_profile(raw) == expected
    assert profile_room(normalize_profile(raw)) == f"profile:{expected}"


def test_signal_delta_patch_applies_missing_as_no_change_and_null_as_delete() -> None:
    previous = {
        "managed_orders": 2,
        "tradeable": True,
        "legs_order": ["venue_a:ABC/USDT", "venue_b:ABC/USDT"],
        "legs": {
            "venue_a:ABC/USDT": {"contract_id": "venue_a:ABC/USDT", "bid": 100.0, "ask": 101.0},
            "venue_b:ABC/USDT": {"contract_id": "venue_b:ABC/USDT", "bid": 99.0, "ask": 100.0},
        },
    }
    current = deepcopy(previous)
    current["managed_orders"] = 3
    current["legs_order"] = ["venue_a:ABC/USDT"]
    current["legs"] = {
        "venue_a:ABC/USDT": {"contract_id": "venue_a:ABC/USDT", "bid": 100.5, "ask": 101.5},
    }

    patch = build_signal_delta_patch(previous, current)

    assert "tradeable" not in patch
    assert patch["managed_orders"] == 3
    assert patch["legs_order"] == ["venue_a:ABC/USDT"]
    assert patch["legs"]["venue_b:ABC/USDT"] is None
    assert patch["legs"]["venue_a:ABC/USDT"]["bid"] == 100.5

    applied = apply_signal_delta_patch(previous, patch)
    assert applied == current


def test_set_profile_joins_and_leaves_profile_rooms(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    socket_server = app.extensions["flux_socketio_server"]

    client = socketio.test_client(app)
    join_ack = client.emit("set_profile", {"profile": "tokenm"}, callback=True)
    assert join_ack["ok"] is True
    assert join_ack["profile"] == "tokenmm"
    assert join_ack["room"] == "profile:tokenmm"
    assert _room_size(socket_server, "profile:tokenmm") == 1

    switch_ack = client.emit("set_profile", {"profile": "sandbox"}, callback=True)
    assert switch_ack["ok"] is False
    assert switch_ack["profile"] == ""
    assert switch_ack["room"] is None
    assert switch_ack["error"]["code"] == "unsupported_profile"
    assert switch_ack["error"]["requested_profile"] == "sandbox"
    assert _room_size(socket_server, "profile:tokenmm") == 0
    assert _room_size(socket_server, "profile:sandbox") == 0

    client.disconnect()


def test_set_profile_clear_unsets_room_with_null_room_ack(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    socket_server = app.extensions["flux_socketio_server"]

    client = socketio.test_client(app)
    _ = client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    clear_ack = client.emit("set_profile", {}, callback=True)

    assert clear_ack["ok"] is True
    assert clear_ack["profile"] == ""
    assert clear_ack["room"] is None
    assert _room_size(socket_server, "profile:tokenmm") == 0
    client.disconnect()


def test_unsupported_profile_ack_and_no_socket_events(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    socket_server = app.extensions["flux_socketio_server"]
    emitter = app.extensions["flux_socket_emitter"]

    client = socketio.test_client(app)
    ack = client.emit("set_profile", {"profile": "unsupported"}, callback=True)
    assert ack["ok"] is False
    assert ack["profile"] == ""
    assert ack["room"] is None
    assert ack["error"]["code"] == "unsupported_profile"
    assert ack["error"]["requested_profile"] == "unsupported"
    assert _room_size(socket_server, "profile:unsupported") == 0

    emitter.stop()
    emitter.emit_once(profile="unsupported")
    emitter.emit_once()
    assert _take_socket_packets(client) == []
    client.disconnect()


def test_socket_emitter_emits_minimum_tokenmm_payload_shapes(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]
    client = socketio.test_client(app)
    _ = client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    emitter.stop()

    emitter.emit_once(profile="tokenmm")

    received = _take_socket_packets(client)
    payloads: dict[str, dict] = {}
    ordered_seqs: list[int] = []
    for packet in received:
        name = packet["name"]
        payload = packet["args"][0]
        payloads[name] = payload
        ordered_seqs.append(payload["seq"])

    assert set(payloads.keys()) == {"market_update", "signal_delta", "trade_update"}
    assert ordered_seqs == sorted(ordered_seqs)

    market_payload = payloads["market_update"]
    assert market_payload["profile"] == "tokenmm"
    assert isinstance(market_payload["server_ts_ms"], int)
    assert isinstance(market_payload["seq"], int)
    assert set(market_payload.keys()) >= {"alerts", "strategies"}

    signal_payload = payloads["signal_delta"]
    assert signal_payload["profile"] == "tokenmm"
    assert signal_payload["strategy_id"] == flux_config.identity.strategy_id
    assert isinstance(signal_payload["seq"], int)
    assert isinstance(signal_payload["server_ts_ms"], int)
    assert isinstance(signal_payload["patch"], dict)

    trade_payload = payloads["trade_update"]
    assert trade_payload["profile"] == "tokenmm"
    assert trade_payload["strategy_id"] == flux_config.identity.strategy_id
    assert trade_payload["op"] == "upsert"
    assert trade_payload["row_id"] == "trade-001"
    assert trade_payload["version"] == 1
    assert isinstance(trade_payload["trade"], dict)


def test_socket_emitter_second_poll_with_no_changes_emits_no_extra_events(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]
    client = socketio.test_client(app)
    _ = client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    emitter.stop()

    emitter.emit_once(profile="tokenmm")
    first_packets = _take_socket_packets(client)
    assert {packet["name"] for packet in first_packets} == SOCKET_EVENT_NAMES

    emitter.emit_once(profile="tokenmm")
    second_packets = _take_socket_packets(client)
    assert second_packets == []


def test_trade_delete_event_emits_once_and_is_reconnect_safe(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]

    client = socketio.test_client(app)
    _ = client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    emitter.stop()

    emitter.emit_once(profile="tokenmm")
    _ = _take_socket_packets(client)

    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-001",
                "seq": 2,
                "version": 2,
                "ts_ms": 1700000000400,
                "op": "delete",
            },
        ],
    )

    emitter.emit_once(profile="tokenmm")
    delete_packets = _take_socket_packets(client)
    trade_packets = [packet for packet in delete_packets if packet["name"] == "trade_update"]
    assert len(trade_packets) == 1
    trade_payload = trade_packets[0]["args"][0]
    assert trade_payload["op"] == "delete"
    assert trade_payload["row_id"] == "trade-001"
    assert trade_payload["version"] == 2
    assert trade_payload["trade"] is None

    client.disconnect()
    reconnect_client = socketio.test_client(app)
    _ = reconnect_client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    emitter.stop()
    emitter.emit_once(profile="tokenmm")
    assert _take_socket_packets(reconnect_client) == []
    reconnect_client.disconnect()
