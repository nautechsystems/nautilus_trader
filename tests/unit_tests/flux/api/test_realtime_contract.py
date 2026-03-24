from __future__ import annotations

from typing import Any

from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.keys import FluxRedisKeys


REALTIME_EVENT_NAME = "realtime_event"


def _seed_required_schema_keys(redis_client, flux_config) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1_700_000_000_000},
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


def _seed_socket_rows(redis_client, flux_config, contract_catalog) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    for contract in contract_catalog:
        base, quote = contract.symbol.split("/", maxsplit=1)
        redis_client.set_json(
            keys.market_last(exchange=contract.exchange, base=base, quote=quote),
            {"bid": 100.0, "ask": 101.0, "ts_ms": 1_700_000_000_100},
        )

    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-001",
                "seq": 1,
                "version": 1,
                "ts_ms": 1_700_000_000_200,
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
                "ts_ms": 1_700_000_000_300,
                "message": "sample-alert",
            },
        ],
    )


def _take_realtime_packets(client) -> list[dict[str, Any]]:
    return [packet for packet in client.get_received() if packet.get("name") == REALTIME_EVENT_NAME]


def _take_legacy_packets(client) -> list[dict[str, Any]]:
    return [
        packet
        for packet in client.get_received()
        if packet.get("name") in {"market_update", "signal_delta", "trade_update"}
    ]


def _signals_snapshot(
    app,
    *,
    profile: str | None = "tokenmm",
    query: dict[str, Any] | None = None,
) -> dict[str, Any]:
    with app.test_client() as client:
        query_string: dict[str, Any] = {"contract_version": 2}
        if profile is not None:
            query_string["profile"] = profile
        if query is not None:
            query_string.update(query)
        response = client.get(
            "/api/v1/signals",
            query_string=query_string,
        )
        assert response.status_code == 200
        return response.get_json()


def _trades_snapshot(
    app,
    *,
    profile: str = "tokenmm",
    query: dict[str, Any] | None = None,
) -> dict[str, Any]:
    with app.test_client() as client:
        query_string = {"profile": profile, "contract_version": 2}
        if query is not None:
            query_string.update(query)
        response = client.get(
            "/api/v1/trades",
            query_string=query_string,
        )
        assert response.status_code == 200
        return response.get_json()


def _alerts_snapshot(
    app,
    *,
    profile: str = "tokenmm",
    query: dict[str, Any] | None = None,
) -> dict[str, Any]:
    with app.test_client() as client:
        query_string = {"profile": profile, "contract_version": 2}
        if query is not None:
            query_string.update(query)
        response = client.get(
            "/api/v1/alerts",
            query_string=query_string,
        )
        assert response.status_code == 200
        return response.get_json()


def _subscribe_without_background_emitter(
    socket_client,
    emitter,
    payload: dict[str, Any],
) -> dict[str, Any]:
    original_start = emitter.start
    emitter.start = lambda: None  # type: ignore[method-assign]
    try:
        return socket_client.emit("subscribe", payload, callback=True)
    finally:
        emitter.start = original_start


def _set_legacy_profile_without_background_emitter(
    socket_client,
    emitter,
    *,
    profile: str = "tokenmm",
) -> dict[str, Any]:
    original_start = emitter.start
    emitter.start = lambda: None  # type: ignore[method-assign]
    try:
        return socket_client.emit("set_profile", {"profile": profile}, callback=True)
    finally:
        emitter.start = original_start


def _standard_subscribe_payload(
    snapshot_body: dict[str, Any],
    *,
    surface: str,
    profile: str = "tokenmm",
    contract_version: int = 2,
) -> dict[str, Any]:
    realtime = snapshot_body["data"]["realtime"]
    return {
        "contract_version": contract_version,
        "surface": surface,
        "profile": profile,
        "surface_query_key": realtime["surface_query_key"],
        "stream_id": realtime["stream_id"],
        "snapshot_revision": realtime["snapshot_revision"],
        "resume_from_seq": realtime["last_seq"],
    }


def test_signals_snapshot_contract_version_two_exposes_realtime_metadata(
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

    body = _signals_snapshot(app)
    realtime = body["data"]["realtime"]

    assert realtime["contract_version"] == 2
    assert realtime["surface"] == "signal"
    assert realtime["profile"] == "tokenmm"
    assert realtime["surface_query_key"]
    assert realtime["stream_id"]
    assert realtime["snapshot_revision"] == 1
    assert realtime["last_seq"] == 0
    assert realtime["capabilities"]["recovery_mode"] == "invalidate_only"
    assert realtime["capabilities"]["replay_supported"] is False


def test_alerts_snapshot_contract_version_two_exposes_realtime_metadata(
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

    body = _alerts_snapshot(app)
    realtime = body["data"]["realtime"]

    assert realtime["contract_version"] == 2
    assert realtime["surface"] == "alerts"
    assert realtime["profile"] == "tokenmm"
    assert realtime["surface_query_key"]
    assert realtime["stream_id"]
    assert realtime["snapshot_revision"] == 1
    assert realtime["last_seq"] == 0
    assert realtime["capabilities"]["recovery_mode"] == "invalidate_only"
    assert realtime["capabilities"]["replay_supported"] is False


def test_alerts_noncanonical_queries_withhold_realtime_metadata(
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

    for query in (
        {"limit": 10},
        {"offset": 1},
        {"strategy": flux_config.identity.strategy_id},
    ):
        body = _alerts_snapshot(app, query=query)
        assert "realtime" not in body["data"]


def test_signals_noncanonical_queries_withhold_realtime_metadata(
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

    explicit_strategy = _signals_snapshot(
        app,
        query={"profile": "tokenmm", "strategy": flux_config.identity.strategy_id},
    )
    assert "realtime" not in explicit_strategy["data"]

    unscoped = _signals_snapshot(app, profile=None)
    assert "realtime" not in unscoped["data"]


def test_signals_snapshot_withholds_realtime_metadata_when_standard_rollout_denies_subscribe(
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
    rollout = app.extensions["flux_realtime_rollout"]

    baseline = _signals_snapshot(app)
    assert "realtime" in baseline["data"]

    rollout["hard_kill_switch"] = True
    kill_switched = _signals_snapshot(app)
    assert "realtime" not in kill_switched["data"]

    rollout["hard_kill_switch"] = False
    rollout["supported_contract_versions"] = {999}
    unsupported_version = _signals_snapshot(app)
    assert "realtime" not in unsupported_version["data"]

    rollout["supported_contract_versions"] = {2}
    rollout["surface_canary_profiles"]["signal"] = set()
    canary_denied = _signals_snapshot(app)
    assert "realtime" not in canary_denied["data"]


def test_standard_contract_polling_only_transport_subscribes_and_receives_heartbeat(
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
    snapshot = _signals_snapshot(app)

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal"),
    )

    assert ack["accepted"] is True
    assert ack["contract_version"] == 2
    assert ack["surface"] == "signal"
    assert ack["accepted_start_seq"] == 0
    assert ack["capabilities"]["recovery_mode"] == "invalidate_only"

    emitter.emit_once(profile="tokenmm")
    realtime_packets = _take_realtime_packets(client)

    assert len(realtime_packets) == 1
    payload = realtime_packets[0]["args"][0]
    assert payload["contract_version"] == 2
    assert payload["surface"] == "signal"
    assert payload["profile"] == "tokenmm"
    assert payload["kind"] == "heartbeat"
    assert payload["stream_id"] == ack["stream_id"]
    assert payload["snapshot_revision"] == ack["snapshot_revision"]
    assert isinstance(payload["server_ts_ms"], int)
    client.disconnect()


def test_standard_alerts_transport_subscribes_and_receives_heartbeat(
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
    snapshot = _alerts_snapshot(app)

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="alerts"),
    )

    assert ack["accepted"] is True
    assert ack["contract_version"] == 2
    assert ack["surface"] == "alerts"
    assert ack["accepted_start_seq"] == 0
    assert ack["capabilities"]["recovery_mode"] == "invalidate_only"

    emitter.emit_once(profile="tokenmm")
    realtime_packets = _take_realtime_packets(client)

    assert len(realtime_packets) == 1
    payload = realtime_packets[0]["args"][0]
    assert payload["contract_version"] == 2
    assert payload["surface"] == "alerts"
    assert payload["profile"] == "tokenmm"
    assert payload["kind"] == "heartbeat"
    assert payload["stream_id"] == ack["stream_id"]
    assert payload["snapshot_revision"] == ack["snapshot_revision"]
    assert isinstance(payload["server_ts_ms"], int)
    client.disconnect()


def test_standard_alerts_change_emits_invalidate_with_summary(
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
    snapshot = _alerts_snapshot(app)

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="alerts"),
    )
    assert ack["accepted"] is True
    _ = _take_realtime_packets(client)

    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.streams[keys.alerts()] = list(redis_client.streams.get(keys.alerts(), [])) + [
        {
            "strategy_id": flux_config.identity.strategy_id,
            "row_id": "alert-002",
            "ts_ms": 1_700_000_000_900,
            "message": "another-alert",
        },
    ]

    emitter.emit_once(profile="tokenmm")
    realtime_packets = _take_realtime_packets(client)

    assert len(realtime_packets) == 1
    payload = realtime_packets[0]["args"][0]
    assert payload["contract_version"] == 2
    assert payload["surface"] == "alerts"
    assert payload["kind"] == "invalidate"
    assert payload["stream_id"] == ack["stream_id"]
    assert payload["snapshot_revision"] == ack["snapshot_revision"]
    assert payload["payload"]["alerts"]["count"] == 2
    assert payload["payload"]["alerts"]["latest_ts_ms"] == 1_700_000_000_900
    client.disconnect()


def test_standard_alerts_subscribe_emits_recovery_required_on_mid_session_withdrawal(
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
    snapshot = _alerts_snapshot(app)

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="alerts"),
    )

    assert ack["accepted"] is True
    assert ack["surface_query_key"] == snapshot["data"]["realtime"]["surface_query_key"]
    assert ack["stream_id"] == snapshot["data"]["realtime"]["stream_id"]
    assert ack["snapshot_revision"] == snapshot["data"]["realtime"]["snapshot_revision"]
    assert ack["last_seq"] == snapshot["data"]["realtime"]["last_seq"]

    app.extensions["flux_realtime_rollout"]["surface_enabled"]["alerts"] = False
    emitter.emit_once(profile="tokenmm")
    packets = _take_realtime_packets(client)

    assert len(packets) == 1
    payload = packets[0]["args"][0]
    assert payload["kind"] == "recovery_required"
    assert payload["reason"] == "capability_withdrawn"
    client.disconnect()


def test_legacy_packets_do_not_advance_standard_surface_cursors(
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

    ack = _set_legacy_profile_without_background_emitter(client, emitter)
    assert ack["ok"] is True
    emitter.emit_once(profile="tokenmm")
    assert _take_legacy_packets(client)

    signals_snapshot = _signals_snapshot(app)
    trades_snapshot = _trades_snapshot(app)
    assert signals_snapshot["data"]["realtime"]["last_seq"] == 0
    assert trades_snapshot["data"]["realtime"]["last_seq"] == 0
    client.disconnect()


def test_standard_signal_delta_batch_is_versioned_and_machine_readable(
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
    snapshot = _signals_snapshot(app)
    client = socketio.test_client(app)

    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal"),
    )
    assert ack["accepted"] is True
    _ = _take_realtime_packets(client)

    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {"bid": 100.5, "ask": 101.5, "ts_ms": 1_700_000_000_900},
    )

    emitter.emit_once(profile="tokenmm")
    realtime_packets = _take_realtime_packets(client)

    assert len(realtime_packets) == 1
    payload = realtime_packets[0]["args"][0]
    assert payload["kind"] == "delta_batch"
    assert payload["contract_version"] == 2
    assert payload["payload"]["signals"][0]["strategy_id"] == flux_config.identity.strategy_id
    assert "patch" in payload["payload"]["signals"][0]
    client.disconnect()


def test_standard_surface_cursors_are_independent_and_surface_specific(
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
    signal_snapshot = _signals_snapshot(app)
    trades_snapshot = _trades_snapshot(app)

    signal_client = socketio.test_client(app)
    trades_client = socketio.test_client(app)
    signal_ack = _subscribe_without_background_emitter(
        signal_client,
        emitter,
        _standard_subscribe_payload(signal_snapshot, surface="signal"),
    )
    trades_ack = _subscribe_without_background_emitter(
        trades_client,
        emitter,
        _standard_subscribe_payload(trades_snapshot, surface="trades"),
    )
    assert signal_ack["accepted"] is True
    assert trades_ack["accepted"] is True
    _ = _take_realtime_packets(signal_client)
    _ = _take_realtime_packets(trades_client)

    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {"bid": 100.5, "ask": 101.5, "ts_ms": 1_700_000_000_900},
    )

    emitter.emit_once(profile="tokenmm")
    signal_after_signal_delta = _signals_snapshot(app)
    trades_after_signal_delta = _trades_snapshot(app)
    assert signal_after_signal_delta["data"]["realtime"]["last_seq"] == 1
    assert trades_after_signal_delta["data"]["realtime"]["last_seq"] == 0

    signal_probe_client = socketio.test_client(app)
    trades_probe_client = socketio.test_client(app)
    signal_probe_ack = _subscribe_without_background_emitter(
        signal_probe_client,
        emitter,
        _standard_subscribe_payload(signal_after_signal_delta, surface="signal"),
    )
    trades_probe_ack = _subscribe_without_background_emitter(
        trades_probe_client,
        emitter,
        _standard_subscribe_payload(trades_after_signal_delta, surface="trades"),
    )
    assert signal_probe_ack["accepted_start_seq"] == 1
    assert trades_probe_ack["accepted_start_seq"] == 0
    signal_probe_client.disconnect()
    trades_probe_client.disconnect()

    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-002",
                "seq": 2,
                "version": 1,
                "ts_ms": 1_700_000_001_000,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "SELL",
                "price": 101.0,
                "qty": 1.0,
            },
        ],
    )

    emitter.emit_once(profile="tokenmm")
    signal_after_trade_delta = _signals_snapshot(app)
    trades_after_trade_delta = _trades_snapshot(app)
    assert signal_after_trade_delta["data"]["realtime"]["last_seq"] == 1
    assert trades_after_trade_delta["data"]["realtime"]["last_seq"] == 1
    signal_client.disconnect()
    trades_client.disconnect()


def test_standard_subscribe_rejects_snapshot_lineage_mismatch_before_any_data_is_sent(
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
    snapshot = _signals_snapshot(app)
    client = socketio.test_client(app)
    payload = _standard_subscribe_payload(snapshot, surface="signal")
    payload["stream_id"] = "signal:stale"

    ack = _subscribe_without_background_emitter(client, emitter, payload)

    assert ack["accepted"] is False
    assert ack["reason"] == "stream_rollover"
    assert _take_realtime_packets(client) == []
    client.disconnect()


def test_standard_subscribe_rejects_missing_snapshot_lineage_before_any_data_is_sent(
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
    snapshot = _signals_snapshot(app)
    client = socketio.test_client(app)
    base_payload = _standard_subscribe_payload(snapshot, surface="signal")

    for field_name, missing_value in (
        ("surface_query_key", ""),
        ("stream_id", ""),
        ("snapshot_revision", None),
    ):
        payload = dict(base_payload)
        payload[field_name] = missing_value
        ack = _subscribe_without_background_emitter(client, emitter, payload)

        assert ack["accepted"] is False
        assert ack["reason"] == "missing_snapshot_lineage"
        assert _take_realtime_packets(client) == []

    client.disconnect()


def test_backend_hard_kill_switch_and_canary_controls_fail_closed_for_standard_only(
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
    snapshot = _signals_snapshot(app)
    app.extensions["flux_realtime_rollout"]["hard_kill_switch"] = True

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal"),
    )

    assert ack["accepted"] is False
    assert ack["reason"] == "backend_kill_switch"
    client.disconnect()

    app.extensions["flux_realtime_rollout"]["hard_kill_switch"] = False
    app.extensions["flux_realtime_rollout"]["surface_canary_profiles"]["signal"] = set()
    canary_client = socketio.test_client(app)
    canary_ack = _subscribe_without_background_emitter(
        canary_client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal"),
    )

    assert canary_ack["accepted"] is False
    assert canary_ack["reason"] == "canary_denied"
    canary_client.disconnect()


def test_backend_hard_kill_switch_and_canary_do_not_break_legacy_clients(
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
    app.extensions["flux_realtime_rollout"]["hard_kill_switch"] = True
    app.extensions["flux_realtime_rollout"]["surface_canary_profiles"]["signal"] = set()
    client = socketio.test_client(app)

    original_start = emitter.start
    emitter.start = lambda: None  # type: ignore[method-assign]
    try:
        ack = client.emit("set_profile", {"profile": "tokenmm"}, callback=True)
    finally:
        emitter.start = original_start

    assert ack["ok"] is True
    emitter.emit_once(profile="tokenmm")
    legacy_packets = _take_legacy_packets(client)

    assert legacy_packets
    payload = legacy_packets[-1]["args"][0]
    assert "contract_version" not in payload
    client.disconnect()


def test_standard_subscribe_ack_exposes_realtime_metadata_and_mid_session_withdrawal(
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
    snapshot = _signals_snapshot(app)

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal"),
    )

    assert ack["accepted"] is True
    assert ack["surface_query_key"] == snapshot["data"]["realtime"]["surface_query_key"]
    assert ack["stream_id"] == snapshot["data"]["realtime"]["stream_id"]
    assert ack["snapshot_revision"] == snapshot["data"]["realtime"]["snapshot_revision"]
    assert ack["last_seq"] == snapshot["data"]["realtime"]["last_seq"]

    app.extensions["flux_realtime_rollout"]["surface_enabled"]["signal"] = False
    emitter.emit_once(profile="tokenmm")
    packets = _take_realtime_packets(client)

    assert len(packets) == 1
    payload = packets[0]["args"][0]
    assert payload["kind"] == "recovery_required"
    assert payload["reason"] == "capability_withdrawn"
    client.disconnect()


def test_trades_snapshot_contract_version_two_uses_standard_stream_cursor_for_lineage(
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
    snapshot = _trades_snapshot(app)
    realtime = snapshot["data"]["realtime"]

    assert snapshot["data"]["last_seq"] == 1
    assert realtime["last_seq"] == 0

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="trades"),
    )

    assert ack["accepted"] is True
    assert ack["accepted_start_seq"] == realtime["last_seq"]
    assert ack["last_seq"] == realtime["last_seq"]
    client.disconnect()


def test_trades_noncanonical_queries_withhold_realtime_metadata(
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

    for query in (
        {"limit": 10},
        {"offset": 1},
        {"sort": "asc"},
        {"coin": "ABC"},
    ):
        body = _trades_snapshot(app, query=query)
        assert "realtime" not in body["data"]


def test_trades_snapshot_withholds_realtime_metadata_when_standard_rollout_denies_subscribe(
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
    rollout = app.extensions["flux_realtime_rollout"]

    baseline = _trades_snapshot(app)
    assert "realtime" in baseline["data"]

    rollout["surface_enabled"]["trades"] = False
    disabled_surface = _trades_snapshot(app)
    assert "realtime" not in disabled_surface["data"]

    rollout["surface_enabled"]["trades"] = True
    rollout["surface_canary_profiles"]["trades"] = set()
    canary_denied = _trades_snapshot(app)
    assert "realtime" not in canary_denied["data"]


def test_trades_unsubscribable_profile_withholds_realtime_metadata(
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

    body = _trades_snapshot(app, profile="sandbox")
    assert body["data"]["rows"]
    assert "realtime" not in body["data"]

    client = socketio.test_client(app)
    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        {
            "contract_version": 2,
            "surface": "trades",
            "profile": "sandbox",
            "surface_query_key": "trades|profile=sandbox|strategy_ids=strategy_01",
            "stream_id": "trades:sandbox:strategy_01",
            "snapshot_revision": 1,
            "resume_from_seq": 0,
        },
    )

    assert ack["accepted"] is False
    assert ack["reason"] == "unsupported_profile"
    client.disconnect()


def test_standard_trades_gap_emits_recovery_required_instead_of_silent_drift(
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
    snapshot = _trades_snapshot(app)
    client = socketio.test_client(app)

    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="trades"),
    )
    assert ack["accepted"] is True
    _ = _take_realtime_packets(client)

    keys = FluxRedisKeys.from_identity(flux_config.identity)
    emitter._trade_scan_limit = 1
    redis_client.add_stream_rows(
        keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-gap",
                "seq": 3,
                "version": 1,
                "ts_ms": 1_700_000_000_500,
                "exchange": "venue_a",
                "symbol": "ABC/USDT",
                "side": "BUY",
                "price": 101.0,
                "qty": 1.0,
            },
        ],
    )

    emitter.emit_once(profile="tokenmm")
    packets = _take_realtime_packets(client)

    assert len(packets) == 1
    payload = packets[0]["args"][0]
    assert payload["surface"] == "trades"
    assert payload["kind"] == "recovery_required"
    assert payload["reason"] == "trade_gap"
    client.disconnect()


def test_backend_rejects_unsupported_contract_versions_explicitly(
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
    snapshot = _signals_snapshot(app)
    client = socketio.test_client(app)

    ack = _subscribe_without_background_emitter(
        client,
        emitter,
        _standard_subscribe_payload(snapshot, surface="signal", contract_version=999),
    )

    assert ack["accepted"] is False
    assert ack["reason"] == "unsupported_contract_version"
    client.disconnect()
