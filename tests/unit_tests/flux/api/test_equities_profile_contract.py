from __future__ import annotations

import importlib

import nautilus_trader.flux.api.app as app_module
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.keys import FluxRedisKeys


def _compat_flux_config(flux_config):
    return app_module.FluxConfig(
        mode=flux_config.mode,
        confirm_live=flux_config.confirm_live,
        identity=flux_config.identity,
        redis=flux_config.redis,
        venues=flux_config.venues,
    )


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


def _seed_required_schema_keys_for_strategy(redis_client, flux_config, strategy_id: str) -> None:
    keys = FluxRedisKeys(
        strategy_id=strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
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
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 100.0}])


def test_signals_profile_equities_returns_only_allowlisted_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_03")

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["id"] for row in body["data"]["strategies"]] == [
        flux_config.identity.strategy_id,
        "strategy_02",
    ]


def test_params_profile_equities_does_not_discover_unallowlisted_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    tertiary_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "2.0"})
    redis_client.set_hash_json(tertiary_keys.params_hash_key(), {"qty": "3.0"})

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["strategy_id"] for row in body["data"]] == [
        flux_config.identity.strategy_id,
        "strategy_02",
    ]


def test_balances_profile_equities_aggregates_cash_and_positions(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "2.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "exchange": "venue_a",
                    "account": "main",
                    "asset": "USDT",
                    "free": "100",
                    "total": "100",
                    "ts_ms": 1_000,
                },
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "kind": "position",
                    "exchange": "venue_a",
                    "instrument_id": "ABCUSDT-LINEAR.BYBIT",
                    "quantity": "2",
                    "side": "LONG",
                },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
                {
                    "strategy_id": "strategy_02",
                    "exchange": "venue_a",
                    "account": "main",
                    "asset": "USDT",
                    "free": "140",
                    "total": "140",
                    "ts_ms": 2_000,
                },
                {
                    "strategy_id": "strategy_02",
                    "kind": "position",
                    "exchange": "venue_a",
                    "instrument_id": "ABCUSDT-LINEAR.BYBIT",
                    "quantity": "1",
                    "side": "SHORT",
                },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    by_row_id = {row["row_id"]: row for row in rows}
    cash_row = by_row_id["equities:cash:venue_a:main:USDT"]
    assert cash_row["free"] == "140"
    assert cash_row["total"] == "140"
    assert cash_row["strategy_id"] == "equities"
    position_row = by_row_id["equities:pos:venue_a:ABCUSDT-LINEAR.BYBIT"]
    assert position_row["signed_qty"] == "1"
    assert position_row["quantity"] == "1"
    assert position_row["side"] == "LONG"
    assert position_row["strategy_id"] == "equities"


def test_namespace_identity_check_import_order_passes_for_flux_api_modules() -> None:
    globals_dict: dict[str, object] = {}

    exec(
        "\n".join(
            [
                "import flux.api.app as a1",
                "import nautilus_trader.flux.api.app as a2",
                "import flux.api.socketio as s1",
                "import nautilus_trader.flux.api.socketio as s2",
                "import flux.api.payloads as p1",
                "import nautilus_trader.flux.api.payloads as p2",
            ],
        ),
        globals_dict,
    )

    assert globals_dict["a1"] is globals_dict["a2"]
    assert globals_dict["s1"] is globals_dict["s2"]
    assert globals_dict["p1"] is globals_dict["p2"]


def test_flux_strategy_package_identity_matches_compat_namespace() -> None:
    root_pkg = importlib.import_module("flux.strategies")
    compat_pkg = importlib.import_module("nautilus_trader.flux.strategies")

    assert root_pkg is compat_pkg


def test_trades_profile_equities_fans_out_allowlisted_strategies_in_global_time_order(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    tertiary_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        primary_keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "t-primary",
                "seq": 11,
                "ts_ms": 3_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        secondary_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-secondary",
                "seq": 12,
                "ts_ms": 2_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        tertiary_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_03",
                "row_id": "t-tertiary",
                "seq": 13,
                "ts_ms": 4_000,
                "coin": "AAPL",
                "exchange": "hyperliquid",
                "side": "sell",
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-primary", "t-secondary"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {
        flux_config.identity.strategy_id,
        "strategy_02",
    }
    assert body["data"]["last_seq"] == 0


def test_socket_profile_equities_joins_room(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )
    socketio = app.extensions["flux_socketio"]
    socket_server = app.extensions["flux_socketio_server"]

    client = socketio.test_client(app)
    join_ack = client.emit("set_profile", {"profile": "equities"}, callback=True)

    assert join_ack["ok"] is True
    assert join_ack["profile"] == "equities"
    assert join_ack["room"] == "profile:equities"
    assert len(socket_server.manager.rooms.get("/", {}).get("profile:equities") or {}) == 1

    client.disconnect()
