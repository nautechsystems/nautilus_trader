from __future__ import annotations

from copy import deepcopy
from typing import Any

import pytest

from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.socketio import FluxSocketEmitter
from nautilus_trader.flux.api.socketio import apply_signal_delta_patch
from nautilus_trader.flux.api.socketio import build_signal_delta_patch
from nautilus_trader.flux.api.socketio import normalize_profile
from nautilus_trader.flux.api.socketio import profile_room
from nautilus_trader.flux.api.socketio import supported_profile_ids
from nautilus_trader.flux.common.keys import FluxRedisKeys


SOCKET_EVENT_NAMES = {"market_update", "signal_delta", "trade_update"}


def _seed_required_schema_keys(redis_client, flux_config) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1700000000000},
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {
            "qty": "1.0",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        keys.fv_stream(),
        [{"strategy_id": flux_config.identity.strategy_id, "fv": 100.0}],
    )


def _seed_required_schema_keys_for_strategy(redis_client, flux_config, strategy_id: str) -> None:
    keys = FluxRedisKeys(
        strategy_id=strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1700000000000},
    )
    redis_client.set_hash_json(
        keys.params_hash_key(),
        {
            "qty": "1.0",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        keys.fv_stream(),
        [{"strategy_id": strategy_id, "fv": 100.0}],
    )


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
                "qty_base": 1.5,
                "qty_venue": 1.5,
                "qty_conversion_status": "identity",
                "qty_conversion_source": "generic:multiplier=1",
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


def _prepare_profile_for_manual_emit(client, emitter, *, profile: str = "tokenmm") -> None:
    _ = client.emit("set_profile", {"profile": profile}, callback=True)
    emitter.stop()
    _ = _take_socket_packets(client)
    with emitter._lock:
        emitter._cleanup_profile_state_locked(profile)


class _TestSocketIO:
    def __init__(self) -> None:
        self.events: list[tuple[str, dict[str, Any], str | None]] = []

    def emit(self, event: str, payload: dict[str, Any], to: str | None = None) -> None:
        self.events.append((event, payload, to))

    def sleep(self, _seconds: float) -> None:
        return


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


def test_supported_profile_ids_include_registered_strategy_sets() -> None:
    assert supported_profile_ids() == ("equities", "tokenmm")


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
    _prepare_profile_for_manual_emit(client, emitter)

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


def test_socket_emitter_emits_seq_less_trade_rows_via_ts_seq_fallback(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
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
                "row_id": "trade-seqless-1",
                "version": 1,
                "ts_ms": 1700000000200,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "BUY",
                "price": 100.0,
                "qty": 1.0,
                "qty_base": 1.0,
                "qty_venue": 1.0,
                "qty_conversion_status": "identity",
                "qty_conversion_source": "generic:multiplier=1",
            },
        ],
    )

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
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")

    received = _take_socket_packets(client)
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]
    assert len(trade_packets) == 1
    trade_payload = trade_packets[0]["args"][0]
    assert trade_payload["row_id"] == "trade-seqless-1"
    assert trade_payload["op"] == "upsert"
    assert isinstance(trade_payload["trade"], dict)
    assert trade_payload["trade"]["seq"] == 1700000000200
    client.disconnect()


def test_socket_emitter_trade_update_projects_base_qty_when_explicit_fields_are_present(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-okx-001",
                "seq": 5,
                "version": 1,
                "ts_ms": 1700000000200,
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "exchange": "okx",
                "side": "BUY",
                "price": "0.012736",
                "qty": "100",
                "qty_base": "1000",
                "qty_venue": "100",
                "qty_conversion_status": "exact_multiplier",
                "qty_conversion_source": "generic:multiplier",
            },
        ],
    )

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
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")

    received = _take_socket_packets(client)
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]
    assert len(trade_packets) == 1
    packet_payload = trade_packets[0]["args"][0]
    trade_payload = packet_payload["trade"]
    assert packet_payload["row_id"] == "trade-okx-001"
    assert trade_payload["qty"] == "1000"
    assert trade_payload["qty_base"] == "1000"
    assert trade_payload["qty_venue"] == "100"
    assert trade_payload["qty_conversion_status"] == "exact_multiplier"
    assert trade_payload["row_id"] == "trade-okx-001"
    client.disconnect()


def test_socket_emitter_tokenmm_requires_recovery_when_trade_rows_lack_normalized_qty(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-legacy-001",
                "seq": 5,
                "version": 1,
                "ts_ms": 1700000000200,
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "exchange": "okx",
                "side": "BUY",
                "price": "0.012736",
                "qty": "100",
            },
        ],
    )

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
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")

    received = _take_socket_packets(client)
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]
    market_packets = [packet for packet in received if packet["name"] == "market_update"]

    assert trade_packets == []
    assert len(market_packets) == 1
    assert market_packets[0]["args"][0]["recovery"] == {"required": True, "reason": "trade_gap"}
    client.disconnect()


def test_socket_emitter_tokenmm_requires_recovery_when_legacy_rows_fall_outside_scan_window(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-legacy-001",
                "seq": 1,
                "version": 1,
                "ts_ms": 1_700_000_000_000,
                "qty": "1",
            },
            *[
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "row_id": f"trade-{seq:04d}",
                    "seq": seq,
                    "version": 1,
                    "ts_ms": 1_700_000_000_000 + seq,
                    "qty": "1",
                    "qty_base": "1",
                    "qty_venue": "1",
                }
                for seq in range(2, 2_002)
            ],
        ],
    )

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
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")

    received = _take_socket_packets(client)
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]
    market_packets = [packet for packet in received if packet["name"] == "market_update"]

    assert trade_packets == []
    assert len(market_packets) == 1
    assert market_packets[0]["args"][0]["recovery"] == {"required": True, "reason": "trade_gap"}
    client.disconnect()


def test_socket_emitter_tokenmm_market_update_reports_changed_allowlisted_signals(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "trade-002",
                "seq": 2,
                "version": 1,
                "ts_ms": 1_700_000_000_250,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "SELL",
                "price": 100.5,
                "qty": 1.0,
                "qty_base": 1.0,
                "qty_venue": 1.0,
                "qty_conversion_status": "identity",
                "qty_conversion_source": "generic:multiplier=1",
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]
    client = socketio.test_client(app)
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")
    received = _take_socket_packets(client)
    market_packets = [packet for packet in received if packet["name"] == "market_update"]
    signal_packets = [packet for packet in received if packet["name"] == "signal_delta"]
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]

    assert len(market_packets) == 1
    market_payload = market_packets[0]["args"][0]
    assert market_payload["strategies"]["changed"] == [
        flux_config.identity.strategy_id,
        "strategy_02",
    ]
    assert {packet["args"][0]["strategy_id"] for packet in signal_packets} == {
        flux_config.identity.strategy_id,
        "strategy_02",
    }
    assert {packet["args"][0]["strategy_id"] for packet in trade_packets} == {
        flux_config.identity.strategy_id,
        "strategy_02",
    }
    client.disconnect()


def test_socket_emitter_tokenmm_market_update_reports_alert_changes_from_secondary_strategy(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]
    client = socketio.test_client(app)
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")
    _ = _take_socket_packets(client)

    redis_client.add_stream_rows(
        strategy_02_keys.alerts(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "alert-002",
                "ts_ms": 1_700_000_000_400,
                "message": "secondary-alert",
            },
        ],
    )

    emitter.emit_once(profile="tokenmm")
    received = _take_socket_packets(client)
    market_packets = [packet for packet in received if packet["name"] == "market_update"]
    signal_packets = [packet for packet in received if packet["name"] == "signal_delta"]
    trade_packets = [packet for packet in received if packet["name"] == "trade_update"]

    assert len(market_packets) == 1
    assert signal_packets == []
    assert trade_packets == []
    market_payload = market_packets[0]["args"][0]
    assert market_payload["alerts"] == {
        "count": 2,
        "latest_ts_ms": 1_700_000_000_400,
    }
    assert market_payload["strategies"]["changed"] == []
    client.disconnect()


def test_socket_emitter_tokenmm_trade_fanout_keeps_same_seq_and_ts_across_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_socket_rows(redis_client, flux_config, contract_catalog)
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    emitter = app.extensions["flux_socket_emitter"]
    client = socketio.test_client(app)
    _prepare_profile_for_manual_emit(client, emitter)

    emitter.emit_once(profile="tokenmm")
    _ = _take_socket_packets(client)

    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "trade-002-shared-seq",
                "seq": 1,
                "version": 1,
                "ts_ms": 1_700_000_000_200,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "SELL",
                "price": 100.5,
                "qty": 1.0,
                "qty_base": 1.0,
                "qty_venue": 1.0,
                "qty_conversion_status": "identity",
                "qty_conversion_source": "generic:multiplier=1",
            },
        ],
    )

    emitter.emit_once(profile="tokenmm")
    second_packets = _take_socket_packets(client)
    second_trade_packets = [packet for packet in second_packets if packet["name"] == "trade_update"]
    assert len(second_trade_packets) == 1
    second_trade_payload = second_trade_packets[0]["args"][0]
    assert second_trade_payload["row_id"] == "trade-002-shared-seq"
    assert second_trade_payload["strategy_id"] == "strategy_02"
    assert second_trade_payload["trade"]["seq"] == 1

    client.disconnect()


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
    _prepare_profile_for_manual_emit(client, emitter)

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
    assert trade_payload["seq"] == 2
    assert trade_payload["version"] == 2
    assert trade_payload["trade"] is None

    client.disconnect()
    reconnect_client = socketio.test_client(app)
    _prepare_profile_for_manual_emit(reconnect_client, emitter)
    emitter.emit_once(profile="tokenmm")
    reconnect_packets = _take_socket_packets(reconnect_client)
    reconnect_trade_packets = [
        packet for packet in reconnect_packets if packet["name"] == "trade_update"
    ]
    assert len(reconnect_trade_packets) == 1
    reconnect_trade_payload = reconnect_trade_packets[0]["args"][0]
    assert reconnect_trade_payload["op"] == "delete"
    assert reconnect_trade_payload["row_id"] == "trade-001"
    assert reconnect_trade_payload["seq"] == 2
    assert reconnect_trade_payload["version"] == 2
    assert reconnect_trade_payload["trade"] is None
    reconnect_client.disconnect()


def test_profile_refcounts_keep_emitter_active_until_last_disconnect_then_cleanup(
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

    client_a = socketio.test_client(app)
    client_b = socketio.test_client(app)
    assert client_a.emit("set_profile", {"profile": "tokenmm"}, callback=True)["ok"] is True
    assert client_b.emit("set_profile", {"profile": "tokenmm"}, callback=True)["ok"] is True
    emitter.stop()
    emitter.emit_once(profile="tokenmm")

    assert emitter._profile_refcounts["tokenmm"] == 2
    assert "tokenmm" in emitter._signal_by_profile
    assert "tokenmm" in emitter._trade_cursor_by_profile
    assert "tokenmm" in emitter._alerts_by_profile

    client_a.disconnect()
    assert emitter._profile_refcounts["tokenmm"] == 1
    assert "tokenmm" in emitter._signal_by_profile

    client_b.disconnect()
    assert "tokenmm" not in emitter._profile_refcounts
    assert "tokenmm" not in emitter._seq_by_profile
    assert "tokenmm" not in emitter._signal_by_profile
    assert "tokenmm" not in emitter._trade_cursor_by_profile
    assert "tokenmm" not in emitter._alerts_by_profile
    assert emitter._active_profiles() == []


def test_emitter_emit_once_is_idle_without_active_profiles_and_skips_store_reads(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
    monkeypatch,
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
    emitter = app.extensions["flux_socket_emitter"]
    store = emitter._store
    emitter.stop()

    def _unexpected_store_call(*_args, **_kwargs):
        raise AssertionError("store read should not run without active profiles")

    monkeypatch.setattr(store, "load_signals_payload", _unexpected_store_call)
    monkeypatch.setattr(store, "load_trades_rows", _unexpected_store_call)
    monkeypatch.setattr(store, "load_alerts_rows", _unexpected_store_call)
    monkeypatch.setattr(store, "alerts_stream_len", _unexpected_store_call)

    assert emitter._active_profiles() == []
    emitter.emit_once()


def test_emitter_isolates_profile_failures_with_logging_and_backoff(caplog) -> None:
    class _Store:
        def __init__(self) -> None:
            self.signal_calls: dict[str, int] = {}

        def load_signals_payload(self, strategy_id: str, metadata: Any) -> dict[str, Any]:
            _ = metadata
            self.signal_calls[strategy_id] = self.signal_calls.get(strategy_id, 0) + 1
            if strategy_id == "strategy_bad":
                raise RuntimeError("boom")
            return {
                "id": strategy_id,
                "meta": {"strategy_id": strategy_id},
                "legs": {},
            }

        def load_trades_rows(
            self,
            strategy_id: str,
            *,
            limit: int,
            since_ms: int | None,
            since_seq: int | None = None,
            scan_limit: int | None = None,
        ) -> list[dict[str, Any]]:
            _ = strategy_id, limit, since_ms, since_seq, scan_limit
            return []

        def load_alerts_rows(self, strategy_id: str, *, limit: int) -> list[dict[str, Any]]:
            _ = strategy_id, limit
            return []

        def alerts_stream_len(self, strategy_id: str) -> int:
            _ = strategy_id
            return 0

    socketio = _TestSocketIO()
    store = _Store()
    strategy_map = {
        "healthy": "strategy_ok",
        "broken": "strategy_bad",
    }
    emitter = FluxSocketEmitter(
        socketio=socketio,
        store=store,
        metadata_resolver=lambda strategy_id: {"strategy_id": strategy_id},
        strategy_resolver=lambda profile: strategy_map.get(profile),
        poll_interval_s=0.25,
    )
    emitter.acquire_profile("healthy")
    emitter.acquire_profile("broken")

    caplog.set_level("ERROR")
    emitter.emit_once()

    healthy_events = [event for event in socketio.events if event[2] == "profile:healthy"]
    assert healthy_events
    assert store.signal_calls["strategy_ok"] == 1
    assert store.signal_calls["strategy_bad"] == 1
    assert any(
        "profile=broken strategy_id=strategy_bad" in record.getMessage()
        for record in caplog.records
    )

    emitter.emit_once()
    assert store.signal_calls["strategy_ok"] == 2
    assert store.signal_calls["strategy_bad"] == 1
