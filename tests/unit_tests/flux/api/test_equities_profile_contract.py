from __future__ import annotations

import importlib

import nautilus_trader.flux.api.app as app_module
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.common.config import FluxConfig
from nautilus_trader.flux.common.config import FluxIdentityConfig
from nautilus_trader.flux.common.config import FluxRedisConfig
from nautilus_trader.flux.common.config import FluxVenuesConfig
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


def test_signals_profile_equities_emits_makerv4_quote_snapshot(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "hedge_paused",
            "ts_ms": 1_700_000_000_000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
                "hedge_leg": "AAPL.NASDAQ",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "effective_spread_bps": 6.5,
                    "quoted_spread_bps": 8.0,
                    "expected_maker_fee_bps": 0.25,
                    "assumed_hedge_fee_bps": 1.0,
                    "hedge_ready": False,
                    "hedge_route": "SMART",
                    "effective_account_source": "userRole.master",
                    "hedge_disabled_reason": "stale_quote",
                    "ibkr_quote_age_ms": 1200,
                },
            },
        },
    )
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(keys.balances_snapshot(), [])
    redis_client.add_stream_rows(keys.fv_stream(), [{"strategy_id": strategy_id, "fv": 255.8}])
    redis_client.set_json(
        keys.market_last(
            "hyperliquid",
            "AAPL",
            "USD",
            instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
        ),
        {
            "exchange": "hyperliquid",
            "symbol": "AAPL/USD",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "bid": 255.7,
            "ask": 255.9,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        keys.market_last("ibkr", "AAPL", "USD", instrument_id="AAPL.NASDAQ"),
        {
            "exchange": "ibkr",
            "symbol": "AAPL/USD",
            "instrument_id": "AAPL.NASDAQ",
            "bid": 255.6,
            "ask": 255.8,
            "ts_ms": 1_700_000_000_001,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["strategies"][0]
    assert row["strategy_family"] == "maker_v4"
    assert row["maker_v4"]["quote_snapshot"]["maker_leg"]["venue"] == "HYPERLIQUID"
    assert row["maker_v4"]["quote_snapshot"]["hedge_leg"]["venue"] == "IBKR"
    assert row["maker_v4"]["quote_snapshot"]["effective_spread_bps"] == 6.5


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


def test_balances_profile_equities_keeps_hyperliquid_usdc_collateral_for_usd_perps(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "accounts": [
                    {
                        "account_id": "hyperliquid-main",
                        "events": [
                            {
                                "account_id": "hyperliquid-main",
                                "ts_ms": 1_700_000_000_123,
                                "balances": [
                                    {
                                        "currency": "USDC",
                                        "free": "250.5",
                                        "locked": "0",
                                        "total": "250.5",
                                    },
                                ],
                            },
                        ],
                    },
                ],
            },
        ],
    )

    app = create_flux_api_app(
        _compat_flux_config(flux_config),
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["count"] == 1
    cash_row = body["data"]["rows"][0]
    assert cash_row["asset"] == "USDC"
    assert cash_row["exchange"] == "hyperliquid"
    assert cash_row["strategy_id"] == "equities"


def test_balances_profile_equities_includes_hyperliquid_and_ibkr_rows(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    strategy_id = "aapl_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=strategy_id,
            strategy_instance_id=strategy_id,
            trader_id="trader_01",
            external_strategy_id=strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": strategy_id,
                "exchange": "hyperliquid",
                "account": "hyperliquid-main",
                "asset": "USDC",
                "free": "250.5",
                "total": "250.5",
                "ts_ms": 1_700_000_000_100,
            },
            {
                "strategy_id": strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_200,
            },
            {
                "strategy_id": strategy_id,
                "kind": "position",
                "exchange": "ibkr",
                "instrument_id": "AAPL.NASDAQ",
                "quantity": "5",
                "side": "LONG",
                "ts_ms": 1_700_000_000_300,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    venues = {row["exchange"] for row in rows}
    assert venues == {"hyperliquid", "ibkr"}
    assert any(row["asset"] == "USDC" for row in rows)
    assert any(row["asset"] == "USD" for row in rows)
    assert any(row.get("kind") == "position" and row["exchange"] == "ibkr" for row in rows)


def test_balances_profile_equities_marks_shared_ibkr_cash_rows_as_shared_account(
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    primary_strategy_id = "aapl_tradexyz_makerv4"
    secondary_strategy_id = "msft_tradexyz_makerv4"
    flux_config = FluxConfig(
        mode="paper",
        confirm_live=False,
        identity=FluxIdentityConfig(
            namespace="flux",
            schema_version="v1",
            strategy_id=primary_strategy_id,
            strategy_instance_id=primary_strategy_id,
            trader_id="trader_01",
            external_strategy_id=primary_strategy_id,
        ),
        redis=FluxRedisConfig(host="127.0.0.1", port=6380, db=0),
        venues=FluxVenuesConfig(
            execution_venue="hyperliquid",
            reference_venue="ibkr",
            execution_symbol="AAPL/USD",
            reference_symbol="AAPL/USD",
        ),
    )
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id=secondary_strategy_id,
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "1.0", "bot_on": "0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": primary_strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_100,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": secondary_strategy_id,
                "exchange": "ibkr",
                "account": "U1234567",
                "asset": "USD",
                "free": "1000",
                "total": "1000",
                "ts_ms": 1_700_000_000_200,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            app_module.ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            app_module.ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        strategy_metadata=app_module.StrategyMetadata(
            strategy_class="maker_v4",
            strategy_groups="equities",
            base_asset="AAPL",
            quote_asset="USD",
            param_set="makerv4",
            strategy_family="maker_v4",
            strategy_version="v4",
        ),
        profile_strategy_map={"equities": [primary_strategy_id, secondary_strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
        param_set="makerv4",
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "equities"})
        body = response.get_json()

    assert response.status_code == 200
    ibkr_cash_rows = [
        row
        for row in body["data"]["rows"]
        if row["exchange"] == "ibkr" and row["asset"] == "USD" and row.get("kind") != "position"
    ]
    assert len(ibkr_cash_rows) == 1
    assert ibkr_cash_rows[0]["scope"] == "shared_account"


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
