from __future__ import annotations

import json

import pytest

import nautilus_trader.flux.api.app as app_module
from nautilus_trader.flux.api import DEFAULT_PARAMS_DEFAULTS
from nautilus_trader.flux.api import DEFAULT_PARAMS_SCHEMA
from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.runners.shared.strategy_set import get_strategy_set_descriptor
from nautilus_trader.flux.common.keys import FluxRedisKeys
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_DEFAULTS
from nautilus_trader.flux.common.params import MAKERV3_RUNTIME_PARAM_SCHEMA


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


def test_default_params_aliases_makerv3_runtime_registry() -> None:
    assert DEFAULT_PARAMS_SCHEMA == MAKERV3_RUNTIME_PARAM_SCHEMA
    assert DEFAULT_PARAMS_DEFAULTS == MAKERV3_RUNTIME_PARAM_DEFAULTS
    assert DEFAULT_PARAMS_SCHEMA is not MAKERV3_RUNTIME_PARAM_SCHEMA
    assert DEFAULT_PARAMS_DEFAULTS is not MAKERV3_RUNTIME_PARAM_DEFAULTS
    assert all(
        DEFAULT_PARAMS_SCHEMA[name] is not MAKERV3_RUNTIME_PARAM_SCHEMA[name]
        for name in DEFAULT_PARAMS_SCHEMA
    )


@pytest.mark.parametrize("value", [float("nan"), float("inf"), float("-inf"), "nan", "inf", "-inf"])
def test_create_app_rejects_non_finite_param_defaults(
    value: object,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    with pytest.raises(ValueError, match="finite"):
        create_flux_api_app(
            flux_config,
            redis_client,
            contract_catalog=contract_catalog,
            strategy_metadata=strategy_metadata,
            params_schema=params_schema,
            params_defaults={**params_defaults, "qty": value},
        )


def test_response_encoding_sanitizes_non_finite_values(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    original_build_envelope = app_module.build_envelope

    def _inject_non_finite(**kwargs):
        envelope = original_build_envelope(**kwargs)
        envelope["data"] = {
            "bad": float("nan"),
            "nested": [float("inf"), {"x": float("-inf")}],
        }
        return envelope

    monkeypatch.setattr(app_module, "build_envelope", _inject_non_finite)
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/")
        raw = response.get_data(as_text=True)
        body = json.loads(raw)

    assert response.status_code == 200
    assert response.mimetype == "application/json"
    assert "NaN" not in raw
    assert "Infinity" not in raw
    assert body["data"] == {"bad": None, "nested": [None, {"x": None}]}


def test_readyz_returns_503_with_missing_flux_schema_keys(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02", flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/readyz", headers={"X-Request-Id": "r-1"})
        body = response.get_json()

    assert response.status_code == 503
    assert body["ok"] is False
    assert body["api_version"] == "v1"
    assert body["request_id"] == "r-1"
    assert isinstance(body["timestamp_ms"], int)
    assert body["error"]["code"] == "service_not_ready"
    assert any(item.startswith("flux:v1:") for item in body["error"]["details"]["missing_keys"])


def test_readyz_returns_200_when_flux_schema_is_ready(
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
        profile_strategy_map={
            "tokenmm": [
                flux_config.identity.strategy_id,
                "strategy_02",
                "strategy_03",
            ],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/readyz")
        body = response.get_json()

    assert response.status_code == 200
    assert body["ok"] is True
    assert body["data"]["schema_ready"] is True
    assert all(bool(value) for value in body["data"]["required_keys"].values())


def test_create_app_exposes_registered_strategy_set_descriptors(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    descriptors = app.extensions["flux_strategy_set_descriptors"]

    assert descriptors["equities"] == get_strategy_set_descriptor("equities")
    assert descriptors["tokenmm"] == get_strategy_set_descriptor("tokenmm")


def test_signals_uses_batched_pipeline_for_key_lookup(
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
            {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000000},
        )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals")
        body = response.get_json()

    assert response.status_code == 200
    assert body["ok"] is True
    assert body["data"]["strategies"][0]["id"] == flux_config.identity.strategy_id
    assert redis_client.direct_get_calls == []
    assert redis_client.pipeline_exec_count >= 1
    batch = redis_client.pipeline_batches[-1]
    assert sum(1 for command in batch if command[0] == "get") == 2 + len(contract_catalog)
    assert any(command[0] == "xrevrange" for command in batch)


def test_signals_include_explicit_running_state_from_runner_truth(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.state(),
        {
            "state": "on_stop",
            "bot_on": True,
            "managed_orders": 0,
            "ts_ms": 1_700_000_000_000,
        },
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

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals")
        body = response.get_json()

    assert response.status_code == 200
    strategy = body["data"]["strategies"][0]
    assert strategy["id"] == flux_config.identity.strategy_id
    assert strategy["state"]["state"] == "on_stop"
    assert strategy["running"] is False


def test_healthz_readiness_exception_returns_not_ok_with_error(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    redis_client.pipeline_execute_error = RuntimeError("readiness exploded")
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/healthz")
        body = response.get_json()

    assert response.status_code == 503
    assert body["ok"] is False
    assert body["error"]["code"] == "readiness_probe_failed"
    assert "reason" in body["error"]["details"]
    assert body["error"]["details"]["schema_prefix"] == "flux:v1"


def test_signals_rejects_malformed_strategy_id(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"strategy": "bad id"})
        body = response.get_json()

    assert response.status_code == 400
    assert body["ok"] is False
    assert body["error"]["code"] == "invalid_strategy_id"
    assert redis_client.pipeline_exec_count == 0


def test_signals_metadata_binds_to_requested_strategy_id(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"strategy": "strategy_02"})
        body = response.get_json()

    assert response.status_code == 200
    strategy = body["data"]["strategies"][0]
    assert strategy["id"] == "strategy_02"
    assert strategy["meta"]["strategy_id"] == "strategy_02"


def test_signals_rejects_unallowlisted_tokenmm_strategy_query(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"strategy": "strategy_02"})
        body = response.get_json()

    assert response.status_code == 404
    assert body["ok"] is False
    assert body["error"]["code"] == "unknown_strategy_id"
    assert redis_client.pipeline_exec_count == 0


def test_signals_rejects_unallowlisted_equities_strategy_query(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"equities": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get(
            "/api/v1/signals",
            query_string={"profile": "equities", "strategy": "strategy_02"},
        )
        body = response.get_json()

    assert response.status_code == 404
    assert body["ok"] is False
    assert body["error"]["code"] == "unknown_strategy_id"
    assert redis_client.pipeline_exec_count == 0


def test_trades_delta_uses_timestamp_seq_fallback_for_seq_less_rows(
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
                "row_id": "trade-seqless-older",
                "ts_ms": 1_700_000_000_100,
                "price": "100.0",
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "trade-seqless-newer",
                "ts_ms": 1_700_000_000_200,
                "price": "101.0",
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

    with app.test_client() as client:
        response = client.get(
            "/api/v1/trades/delta",
            query_string={"since_seq": "1700000000150", "limit": "50"},
        )
        body = response.get_json()

    assert response.status_code == 200
    assert body["ok"] is True
    rows = body["data"]["rows"]
    assert len(rows) == 1
    assert rows[0]["row_id"] == "trade-seqless-newer"
    assert rows[0]["seq"] == 1_700_000_000_200
    assert body["data"]["last_seq"] == 1_700_000_000_200
    assert body["data"]["reset_required"] is False


def test_create_app_rejects_invalid_contract_symbol(
    flux_config,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    with pytest.raises(ValueError, match="base/quote"):
        create_flux_api_app(
            flux_config,
            redis_client,
            contract_catalog=(ContractCatalogEntry(exchange="venue_a", symbol="INVALID"),),
            strategy_metadata=StrategyMetadata(
                strategy_class="maker_v3",
                strategy_groups="tokenmm",
                base_asset="ABC",
                quote_asset="USDT",
            ),
            params_schema=params_schema,
            params_defaults=params_defaults,
        )


def test_signals_keeps_distinct_legs_for_same_exchange_different_symbols(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000000},
    )
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="XYZ", quote="USDT"),
        {"bid": 200.0, "ask": 201.0, "ts_ms": 1700000000100},
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),
            ContractCatalogEntry(exchange="venue_a", symbol="XYZ/USDT"),
        ),
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals")
        body = response.get_json()

    assert response.status_code == 200
    legs = body["data"]["strategies"][0]["legs"]
    assert set(legs.keys()) == {"venue_a:ABC/USDT", "venue_a:XYZ/USDT"}
    assert legs["venue_a:ABC/USDT"]["exchange"] == "venue_a"
    assert legs["venue_a:ABC/USDT"]["symbol"] == "ABC/USDT"
    assert legs["venue_a:ABC/USDT"]["mid"] == 100.5
    assert legs["venue_a:XYZ/USDT"]["exchange"] == "venue_a"
    assert legs["venue_a:XYZ/USDT"]["symbol"] == "XYZ/USDT"
    assert legs["venue_a:XYZ/USDT"]["mid"] == 200.5


def test_create_app_rejects_duplicate_contract_catalog_entries_after_normalization(
    flux_config,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    with pytest.raises(ValueError, match="Duplicate contract catalog entry"):
        create_flux_api_app(
            flux_config,
            redis_client,
            contract_catalog=(
                ContractCatalogEntry(exchange="Venue_A", symbol="ABC/USDT"),
                ContractCatalogEntry(exchange="venue_a", symbol="abc/usdt"),
            ),
            strategy_metadata=StrategyMetadata(
                strategy_class="maker_v3",
                strategy_groups="tokenmm",
                base_asset="ABC",
                quote_asset="USDT",
            ),
            params_schema=params_schema,
            params_defaults=params_defaults,
        )


def test_create_app_allows_same_exchange_pair_when_instrument_ids_differ(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-LINEAR.BYBIT",
            ),
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-SPOT.BYBIT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    assert app is not None


def test_create_app_allows_trade_xyz_prefixed_hyperliquid_instrument_id(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
        ),
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    assert app is not None


def test_market_keys_skip_legacy_fallback_when_exchange_pair_is_ambiguous(
    flux_config,
    redis_client,
    params_schema,
    params_defaults,
) -> None:
    store = app_module.FluxApiStore(
        flux_config=flux_config,
        redis_client=redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-LINEAR.BYBIT",
            ),
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-SPOT.BYBIT",
            ),
        ),
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    market_keys = store._market_keys(flux_config.identity.strategy_id)

    assert [fallback for _contract, _primary, fallback in market_keys] == [None, None]
    assert [primary for _contract, primary, _fallback in market_keys] == [
        "flux:v1:market:last:strategy_01:bybit:PLUMEUSDT-LINEAR.BYBIT",
        "flux:v1:market:last:strategy_01:bybit:PLUMEUSDT-SPOT.BYBIT",
    ]


def test_params_profile_fanout_returns_multiple_tokenmm_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys_secondary = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(
        keys_secondary.params_hash_key(),
        {
            "qty": "2.5",
            "bot_on": "0",
            "max_age_ms": "5000",
        },
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        profile_required_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]
    assert isinstance(rows, list)
    by_id = {row["strategy_id"]: row for row in rows}
    assert set(by_id) == {flux_config.identity.strategy_id, "strategy_02"}
    assert by_id["strategy_02"]["params"]["qty"] == 2.5


def test_params_profile_tokenmm_uses_explicit_allowlist_without_discovery(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]
    assert [row["strategy_id"] for row in rows] == ["strategy_02"]


def test_params_includes_running_from_fresh_state_heartbeat(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_010_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(
        primary_keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_hash_json(
        secondary_keys.params_hash_key(),
        {"qty": "2.0", "bot_on": "1", "max_age_ms": "10000"},
    )
    redis_client.set_json(
        primary_keys.state(),
        {"state": "running", "bot_on": False, "managed_orders": 0, "ts_ms": 1_700_000_009_500},
    )
    redis_client.set_json(
        secondary_keys.state(),
        {"state": "running", "bot_on": True, "managed_orders": 0, "ts_ms": 1_700_000_000_000},
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    by_id = {row["strategy_id"]: row for row in body["data"]}
    assert by_id[flux_config.identity.strategy_id]["running"] is True
    assert by_id["strategy_02"]["running"] is False


def test_params_include_persisted_and_effective_bot_on_fields(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_010_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(
        primary_keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "max_age_ms": "10000"},
    )
    redis_client.set_json(
        primary_keys.state(),
        {
            "state": "startup_bot_off",
            "bot_on": False,
            "effective_bot_on": False,
            "persisted_bot_on": True,
            "config_bot_on": True,
            "bot_on_reason": "startup_bot_off",
            "startup_bot_off_active": True,
            "managed_orders": 0,
            "ts_ms": 1_700_000_009_500,
        },
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"][0]
    assert row["params"]["bot_on"] is True
    assert row["persisted_bot_on"] is True
    assert row["config_bot_on"] is True
    assert row["effective_bot_on"] is False
    assert row["bot_on_reason"] == "startup_bot_off"
    assert row["startup_bot_off_active"] is True
    assert row["state"] == "startup_bot_off"
    assert row["running"] is True


def test_params_ignore_stale_state_summary_when_augmenting_bot_on_fields(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_010_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(
        primary_keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "1", "max_age_ms": "10000"},
    )
    redis_client.set_json(
        primary_keys.state(),
        {
            "state": "startup_bot_off",
            "bot_on": False,
            "effective_bot_on": False,
            "persisted_bot_on": True,
            "config_bot_on": True,
            "bot_on_reason": "startup_bot_off",
            "startup_bot_off_active": True,
            "managed_orders": 0,
            "ts_ms": 1_700_000_000_000,
        },
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"][0]
    assert row["params"]["bot_on"] is True
    assert row["persisted_bot_on"] is True
    assert row["config_bot_on"] is True
    assert row["effective_bot_on"] is True
    assert row["bot_on_reason"] == "running"
    assert row["startup_bot_off_active"] is False
    assert row["running"] is False
    assert "state" not in row


def test_store_update_params_records_bot_on_control_revision(
    flux_config,
    redis_client,
    contract_catalog,
    params_schema,
    params_defaults,
) -> None:
    store = app_module.FluxApiStore(
        flux_config=flux_config,
        redis_client=redis_client,
        contract_catalog=contract_catalog,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    result = store.update_params(flux_config.identity.strategy_id, {"bot_on": True})
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    metadata = redis_client.hashes[keys.params_metadata_key()]

    assert result["updated"] == ["bot_on"]
    assert result["params"]["bot_on"] is True
    assert metadata["bot_on_control_revision"].decode("utf-8")


def test_balances_profile_tokenmm_honors_explicit_required_subset(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 100_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "asset": "USDT",
                "free": "10",
                "total": "10",
                "ts_ms": 98_000,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        profile_required_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["missing_required"] == []
    components = {row["strategy_id"]: row for row in body["data"]["components"]}
    assert components[flux_config.identity.strategy_id]["required"] is True
    assert components["strategy_02"]["required"] is False


def test_create_app_rejects_required_ids_outside_profile_allowlist(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    with pytest.raises(ValueError, match="subset"):
        create_flux_api_app(
            flux_config,
            redis_client,
            contract_catalog=contract_catalog,
            strategy_metadata=strategy_metadata,
            profile_strategy_map={"tokenmm": ["strategy_02"]},
            profile_required_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
            params_schema=params_schema,
            params_defaults=params_defaults,
        )


def test_signals_profile_tokenmm_does_not_discover_unallowlisted_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    alt_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(
        alt_keys.params_hash_key(),
        {
            "qty": "2.5",
            "bot_on": "1",
            "max_age_ms": "10000",
        },
    )
    redis_client.set_json(
        alt_keys.state(),
        {"bot_on": True, "managed_orders": 2, "ts_ms": 1700000000000},
    )
    redis_client.set_json(alt_keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        alt_keys.fv_stream(),
        [{"strategy_id": "strategy_02", "fv": 100.0}],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["id"] for row in body["data"]["strategies"]] == [flux_config.identity.strategy_id]


def test_signals_profile_tokenmm_fans_out_allowlisted_strategies_in_order(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_03")
    _seed_required_schema_keys(redis_client, flux_config)
    expected_ids = [
        "strategy_03",
        flux_config.identity.strategy_id,
        "strategy_02",
    ]
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": expected_ids},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["id"] for row in body["data"]["strategies"]] == expected_ids


def test_signals_with_strategy_query_keeps_per_strategy_debug_view_with_tokenmm_profile(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys_for_strategy(redis_client, flux_config, "strategy_02")
    _seed_required_schema_keys(redis_client, flux_config)
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

    with app.test_client() as client:
        response = client.get(
            "/api/v1/signals",
            query_string={"profile": "tokenmm", "strategy": "strategy_02"},
        )
        body = response.get_json()

    assert response.status_code == 200
    assert len(body["data"]["strategies"]) == 1
    assert body["data"]["strategies"][0]["id"] == "strategy_02"


def test_signals_profile_tokenmm_overlays_portfolio_inventory_metadata_onto_rows(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 123_456)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        primary_keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "startup_bot_off",
            "ts_ms": 1_700_000_120_000,
            "pricing_adjustments": [{"type": "inventory_skew", "global_qty_base": "15"}],
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_120_111,
                    "mode": "OFF",
                    "reason": "startup_bot_off",
                },
            },
        },
    )
    redis_client.set_hash_json(
        primary_keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_json(primary_keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        primary_keys.fv_stream(),
        [{"strategy_id": flux_config.identity.strategy_id, "fv": 100.0}],
    )
    redis_client.set_json(
        secondary_keys.state(),
        {
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1_700_000_119_000,
            "pricing_adjustments": [{"type": "inventory_skew", "global_qty_base": "999"}],
            "maker_v3": {
                "quote_snapshot": {
                    "ts_ms": 1_700_000_119_111,
                    "mode": "OFF",
                    "reason": "bot_off",
                },
            },
        },
    )
    redis_client.set_hash_json(
        secondary_keys.params_hash_key(),
        {"qty": "1.0", "bot_on": "0", "max_age_ms": "10000"},
    )
    redis_client.set_json(secondary_keys.balances_snapshot(), [])
    redis_client.add_stream_rows(
        secondary_keys.fv_stream(),
        [{"strategy_id": "strategy_02", "fv": 100.0}],
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "ABC",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "ABC",
                "global_qty_base": "-317104.54289229",
                "global_qty": "-317104.54289229",
                "aggregation_mode": "partial",
                "global_qty_base_complete": False,
                "global_qty_complete": False,
                "missing_required": [],
                "stale_required": ["strategy_02"],
                "null_qty_required": [],
                "degraded": True,
                "ts_ms": 122_000,
                "stale_after_ms": 3_000,
                "components": [
                    {
                        "strategy_id": flux_config.identity.strategy_id,
                        "local_qty_base": "-10",
                        "local_qty": "-10",
                        "local_position_qty_base": "-10",
                        "local_position_qty_venue": "-10",
                        "ts_ms": 122_000,
                        "state": "startup_bot_off",
                    },
                    {
                        "strategy_id": "strategy_02",
                        "local_qty_base": "87589",
                        "local_qty": "87589",
                        "local_position_qty_base": "87589",
                        "local_position_qty_venue": "87589",
                        "ts_ms": 119_500,
                        "state": "bot_off",
                        "stale": True,
                    },
                ],
            },
            "components": [
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "local_qty_base": "-10",
                    "local_qty": "-10",
                    "local_position_qty_base": "-10",
                    "local_position_qty_venue": "-10",
                    "ts_ms": 122_000,
                    "state": "startup_bot_off",
                },
                {
                    "strategy_id": "strategy_02",
                    "local_qty_base": "87589",
                    "local_qty": "87589",
                    "local_position_qty_base": "87589",
                    "local_position_qty_venue": "87589",
                    "ts_ms": 119_500,
                    "state": "bot_off",
                    "stale": True,
                },
            ],
            "balances": {
                "rows": [],
                "totals": {"mv_raw": 0.0, "mv_display": "$0.00"},
            },
            "server_ts_ms": 122_500,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/signals", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = {row["id"]: row for row in body["data"]["strategies"]}

    primary = rows[flux_config.identity.strategy_id]
    assert primary["global_qty_base"] == pytest.approx(-317104.54289229)
    assert primary["global_qty_base_complete"] is False
    assert primary["aggregation_mode"] == "partial"
    assert primary["local_qty_base"] == pytest.approx(-10.0)
    assert primary["mode"] == "OFF"
    assert primary["reason"] == "startup_bot_off"
    assert primary["ts_ms"] == 1_700_000_120_111
    assert primary["pricing_adjustments"][0]["global_qty_base"] == pytest.approx(-317104.54289229)
    assert primary["pricing_adjustments"][0]["aggregation_mode"] == "partial"
    assert primary["pricing_adjustments"][0]["local_qty_base"] == pytest.approx(-10.0)

    secondary = rows["strategy_02"]
    assert secondary["global_qty_base"] == pytest.approx(-317104.54289229)
    assert secondary["global_qty_base_complete"] is False
    assert secondary["aggregation_mode"] == "partial"
    assert secondary["local_qty_base"] == pytest.approx(87589.0)
    assert secondary["pricing_adjustments"][0]["global_qty_base"] == pytest.approx(-317104.54289229)
    assert secondary["pricing_adjustments"][0]["aggregation_mode"] == "partial"
    assert secondary["pricing_adjustments"][0]["local_qty_base"] == pytest.approx(87589.0)


def test_trades_profile_tokenmm_fans_out_allowlisted_strategies_in_global_time_order(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    alt_keys = FluxRedisKeys(
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
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        alt_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-alt",
                "seq": 12,
                "ts_ms": 2_000,
                "coin": "PLUME",
                "exchange": "bybit",
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
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "sell",
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": ["strategy_02", flux_config.identity.strategy_id, "strategy_03"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == [
        "t-tertiary",
        "t-primary",
        "t-alt",
    ]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {
        flux_config.identity.strategy_id,
        "strategy_02",
        "strategy_03",
    }
    assert body["data"]["last_seq"] == 0


def test_trades_profile_tokenmm_keeps_same_ts_and_seq_rows_from_multiple_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    strategy_03_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    shared_seq = 77
    shared_ts_ms = 7_000
    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-02-shared",
                "seq": shared_seq,
                "ts_ms": shared_ts_ms,
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        strategy_03_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_03",
                "row_id": "t-03-shared",
                "seq": shared_seq,
                "ts_ms": shared_ts_ms,
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "sell",
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02", "strategy_03"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-03-shared", "t-02-shared"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {"strategy_02", "strategy_03"}
    assert body["data"]["last_seq"] == 0


def test_trades_with_strategy_query_keeps_per_strategy_debug_view_with_tokenmm_profile(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    alt_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        primary_keys.trades_stream(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "t-primary",
                "seq": 1,
                "ts_ms": 1_000,
            },
        ],
    )
    redis_client.add_stream_rows(
        alt_keys.trades_stream(),
        [{"strategy_id": "strategy_02", "row_id": "t-alt", "seq": 2, "ts_ms": 2_000}],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get(
            "/api/v1/trades",
            query_string={"profile": "tokenmm", "strategy": "strategy_02"},
        )
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-alt"]
    assert {row["strategy_id"] for row in body["data"]["rows"]} == {"strategy_02"}


def test_trades_delta_profile_tokenmm_since_seq_uses_safe_reset_semantics(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    strategy_03_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-02",
                "seq": 1,
                "ts_ms": 1_000,
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "buy",
            },
        ],
    )
    redis_client.add_stream_rows(
        strategy_03_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_03",
                "row_id": "t-03",
                "seq": 1,
                "ts_ms": 2_000,
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "sell",
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02", "strategy_03"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        since_zero_response = client.get(
            "/api/v1/trades/delta",
            query_string={"profile": "tokenmm", "since_seq": 0, "limit": 10},
        )
        since_zero_body = since_zero_response.get_json()
        since_positive_response = client.get(
            "/api/v1/trades/delta",
            query_string={"profile": "tokenmm", "since_seq": 1, "limit": 10},
        )
        since_positive_body = since_positive_response.get_json()

    assert since_zero_response.status_code == 200
    assert since_zero_body["data"]["rows"] == []
    assert since_zero_body["data"]["last_seq"] == 0
    assert since_zero_body["data"]["reset_required"] is False
    assert since_positive_response.status_code == 200
    assert since_positive_body["data"]["rows"] == []
    assert since_positive_body["data"]["last_seq"] == 0
    assert since_positive_body["data"]["reset_required"] is True


def test_trades_delta_profile_tokenmm_after_preserves_oldest_unseen_rows_across_strategies(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    strategy_03_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {"strategy_id": "strategy_02", "row_id": "t-02-1", "seq": 1, "ts_ms": 1_100},
            {"strategy_id": "strategy_02", "row_id": "t-02-2", "seq": 2, "ts_ms": 1_200},
            {"strategy_id": "strategy_02", "row_id": "t-02-3", "seq": 3, "ts_ms": 1_300},
        ],
    )
    redis_client.add_stream_rows(
        strategy_03_keys.trades_stream(),
        [
            {"strategy_id": "strategy_03", "row_id": "t-03-1", "seq": 1, "ts_ms": 1_400},
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02", "strategy_03"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get(
            "/api/v1/trades/delta",
            query_string={"profile": "tokenmm", "after": 1_000, "limit": 2},
        )
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-02-1", "t-02-2"]
    assert [row["strategy_id"] for row in body["data"]["rows"]] == ["strategy_02", "strategy_02"]
    assert body["data"]["reset_required"] is False


def test_trades_delta_profile_tokenmm_after_supports_same_timestamp_replay_cursor(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    strategy_02_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    strategy_03_keys = FluxRedisKeys(
        strategy_id="strategy_03",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    shared_ts_ms = 1_700_000_007_000
    redis_client.add_stream_rows(
        strategy_02_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "trade-a",
                "seq": 1,
                "version": 1,
                "ts_ms": shared_ts_ms,
            },
            {
                "strategy_id": "strategy_02",
                "row_id": "trade-c",
                "seq": 3,
                "version": 1,
                "ts_ms": shared_ts_ms,
            },
        ],
    )
    redis_client.add_stream_rows(
        strategy_03_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_03",
                "row_id": "trade-b",
                "seq": 1,
                "version": 1,
                "ts_ms": shared_ts_ms,
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": ["strategy_02", "strategy_03"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        first_response = client.get(
            "/api/v1/trades/delta",
            query_string={"profile": "tokenmm", "after": shared_ts_ms - 1, "limit": 2},
        )
        first_body = first_response.get_json()
        replay_cursor = first_body["data"]["rows"][-1]
        second_response = client.get(
            "/api/v1/trades/delta",
            query_string={
                "profile": "tokenmm",
                "after": shared_ts_ms,
                "after_row_id": replay_cursor["row_id"],
                "after_version": replay_cursor["version"],
                "limit": 2,
            },
        )
        second_body = second_response.get_json()

    assert first_response.status_code == 200
    assert [row["row_id"] for row in first_body["data"]["rows"]] == ["trade-a", "trade-b"]
    assert second_response.status_code == 200
    assert [row["row_id"] for row in second_body["data"]["rows"]] == ["trade-c"]
    assert second_body["data"]["reset_required"] is False


def test_trades_endpoint_applies_supported_filters_and_sort(
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
                "signal_id": "sig_a",
                "row_id": "t-1",
                "seq": 1,
                "ts_ms": 1_000,
                "coin": "AAA",
                "instrument_id": "AAAUSDT-SPOT.VENUE_A",
                "exchange": "venue_a",
                "side": "buy",
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "signal_id": "sig_b",
                "row_id": "t-2",
                "seq": 2,
                "ts_ms": 2_000,
                "coin": "BBB",
                "instrument_id": "BBBUSDT-LINEAR.VENUE_B",
                "exchange": "venue_b",
                "side": "sell",
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "signal_id": "sig_c",
                "row_id": "t-3",
                "seq": 3,
                "ts_ms": 3_000,
                "coin": "AAA",
                "instrument_id": "AAAUSDT-LINEAR.VENUE_B",
                "exchange": "venue_b",
                "side": "buy",
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

    with app.test_client() as client:
        coin_response = client.get("/api/v1/trades", query_string={"coin": "AAA"})
        coin_body = coin_response.get_json()
        exchange_response = client.get("/api/v1/trades", query_string={"exchange": "venue_b"})
        exchange_body = exchange_response.get_json()
        side_response = client.get("/api/v1/trades", query_string={"side": "buy"})
        side_body = side_response.get_json()
        signal_response = client.get("/api/v1/trades", query_string={"signal_id": "sig_b"})
        signal_body = signal_response.get_json()
        market_type_response = client.get("/api/v1/trades", query_string={"market_type": "perp"})
        market_type_body = market_type_response.get_json()
        sort_asc_response = client.get("/api/v1/trades", query_string={"sort": "asc"})
        sort_asc_body = sort_asc_response.get_json()
        sort_desc_response = client.get("/api/v1/trades", query_string={"sort": "desc"})
        sort_desc_body = sort_desc_response.get_json()

    assert coin_response.status_code == 200
    assert [row["row_id"] for row in coin_body["data"]["rows"]] == ["t-3", "t-1"]
    assert all(row["coin"] == "AAA" for row in coin_body["data"]["rows"])

    assert exchange_response.status_code == 200
    assert [row["row_id"] for row in exchange_body["data"]["rows"]] == ["t-3", "t-2"]
    assert all(row["exchange"] == "venue_b" for row in exchange_body["data"]["rows"])

    assert side_response.status_code == 200
    assert [row["row_id"] for row in side_body["data"]["rows"]] == ["t-3", "t-1"]
    assert all(row["side"] == "buy" for row in side_body["data"]["rows"])

    assert signal_response.status_code == 200
    assert [row["row_id"] for row in signal_body["data"]["rows"]] == ["t-2"]
    assert all(row["signal_id"] == "sig_b" for row in signal_body["data"]["rows"])

    assert market_type_response.status_code == 200
    assert [row["row_id"] for row in market_type_body["data"]["rows"]] == ["t-3", "t-2"]
    assert all(row["product_type"] == "perp" for row in market_type_body["data"]["rows"])

    assert sort_asc_response.status_code == 200
    assert [row["row_id"] for row in sort_asc_body["data"]["rows"]] == ["t-1", "t-2", "t-3"]
    assert sort_asc_body["data"]["sort"] == "ts_ms_asc"

    assert sort_desc_response.status_code == 200
    assert [row["row_id"] for row in sort_desc_body["data"]["rows"]] == ["t-3", "t-2", "t-1"]
    assert sort_desc_body["data"]["sort"] == "ts_ms_desc"


def test_trades_exchange_filter_uses_canonical_venue_for_instrument_only_rows(
    flux_config,
    redis_client,
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
                "row_id": "t-binance-spot",
                "strategy_id": flux_config.identity.strategy_id,
                "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "side": "BUY",
                "price": "0.0105",
                "qty": "1000",
                "ts_ms": 1_700_000_000_100,
            },
        ],
    )
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="binance_spot",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT.BINANCE_SPOT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"exchange": "binance_spot"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-binance-spot"]
    assert body["data"]["rows"][0]["exchange"] == "binance_spot"


def test_trades_response_reports_effective_limit_when_requested_limit_exceeds_cap(
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
                "row_id": f"t-{seq}",
                "seq": seq,
                "ts_ms": seq * 1_000,
            }
            for seq in range(1, 6)
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
        profile_required_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"limit": 999})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["requested_limit"] == 999
    assert body["data"]["effective_limit"] == 200
    assert body["data"]["max_limit"] == 200
    assert body["data"]["limit"] == 200
    assert len(body["data"]["rows"]) == 5


def test_trades_endpoint_filters_scan_full_history_instead_of_recent_2000_only(
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
                "row_id": f"t-{seq}",
                "seq": seq,
                "ts_ms": seq * 1_000,
                "coin": "AAA" if seq == 50 else "BBB",
                "exchange": "venue_a",
                "side": "buy",
            }
            for seq in range(1, 2_106)
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

    with app.test_client() as client:
        response = client.get("/api/v1/trades", query_string={"coin": "AAA", "limit": 50})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-50"]
    assert body["data"]["total"] == 1
    assert body["data"]["has_more"] is False


def test_balances_response_totals_are_authoritative_even_when_rows_are_truncated(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {"strategy_id": flux_config.identity.strategy_id, "row_id": "b-1", "mv_raw": 10.0},
            {"strategy_id": flux_config.identity.strategy_id, "row_id": "b-2", "mv_raw": 20.0},
            {"strategy_id": flux_config.identity.strategy_id, "row_id": "b-3", "mv_raw": 30.0},
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
        profile_required_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"limit": 2})
        body = response.get_json()

    assert response.status_code == 200
    assert len(body["data"]["rows"]) == 2
    assert body["data"]["count"] == 3
    assert body["data"]["total"] == 3
    assert body["data"]["totals"]["mv_raw"] == 60.0
    assert body["data"]["totals"]["mv_display"] == "$60.00"


def test_balances_profile_tokenmm_aggregates_cash_and_positions(
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

    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    by_row_id = {row["row_id"]: row for row in rows}
    cash_row = by_row_id["tokenmm:cash:venue_a:main:USDT"]
    assert cash_row["free"] == "140"
    assert cash_row["total"] == "140"
    assert cash_row["strategy_id"] == "tokenmm"

    position_row = by_row_id["tokenmm:pos:venue_a:ABCUSDT-LINEAR.BYBIT"]
    assert position_row["signed_qty"] == "1"
    assert position_row["quantity"] == "1"
    assert position_row["side"] == "LONG"
    assert position_row["strategy_id"] == "tokenmm"


def test_balances_endpoint_defaults_position_qty_to_base_when_dual_unit_fields_present(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "okx",
                "kind": "position",
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "signed_qty": "343",
                "quantity": "343",
                "signed_qty_venue": "343",
                "quantity_venue": "343",
                "signed_qty_base": "3430",
                "quantity_base": "3430",
                "side": "LONG",
                "qty_conversion_status": "exact_multiplier",
                "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
                "ts_ms": 2_000,
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances")
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    assert len(rows) == 1
    position_row = rows[0]
    assert position_row["signed_qty"] == "3430"
    assert position_row["quantity"] == "3430"
    assert position_row["signed_qty_base"] == "3430"
    assert position_row["quantity_base"] == "3430"
    assert position_row["signed_qty_venue"] == "343"
    assert position_row["quantity_venue"] == "343"
    assert position_row["qty_conversion_status"] == "exact_multiplier"
    assert (
        position_row["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )


def test_balances_profile_tokenmm_aggregates_base_first_position_aliases_and_preserves_venue_fields(
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
                "kind": "position",
                "instrument_id": "ABCUSDT-LINEAR.VENUE_A",
                "signed_qty": "343",
                "quantity": "343",
                "signed_qty_venue": "343",
                "quantity_venue": "343",
                "signed_qty_base": "3430",
                "quantity_base": "3430",
                "side": "LONG",
                "qty_conversion_status": "exact_multiplier",
                "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "venue_a",
                "kind": "position",
                "instrument_id": "ABCUSDT-LINEAR.VENUE_A",
                "signed_qty": "-100",
                "quantity": "100",
                "signed_qty_venue": "-100",
                "quantity_venue": "100",
                "signed_qty_base": "-1000",
                "quantity_base": "1000",
                "side": "SHORT",
                "qty_conversion_status": "exact_multiplier",
                "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    assert len(rows) == 1
    position_row = rows[0]
    assert position_row["row_id"] == "tokenmm:pos:venue_a:ABCUSDT-LINEAR.VENUE_A"
    assert position_row["signed_qty"] == "2430"
    assert position_row["quantity"] == "2430"
    assert position_row["signed_qty_base"] == "2430"
    assert position_row["quantity_base"] == "2430"
    assert position_row["signed_qty_venue"] == "243"
    assert position_row["quantity_venue"] == "243"
    assert position_row["side"] == "LONG"
    assert position_row["qty_conversion_status"] == "exact_multiplier"
    assert (
        position_row["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )


def test_balances_profile_tokenmm_prioritizes_non_zero_rows_before_limit(
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
                "exchange": "bybit",
                "account": "main",
                "asset": "USDT",
                "free": "100",
                "total": "100",
                "ts_ms": 2_000,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "binance_spot",
                "account": "main",
                "asset": "AAA",
                "free": "0",
                "total": "0",
                "ts_ms": 3_000,
            },
            {
                "strategy_id": "strategy_02",
                "exchange": "binance_spot",
                "account": "main",
                "asset": "BBB",
                "free": "0",
                "total": "0",
                "ts_ms": 3_000,
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm", "limit": 1})
        body = response.get_json()

    assert response.status_code == 200
    assert len(body["data"]["rows"]) == 1
    assert body["data"]["rows"][0]["row_id"] == "tokenmm:cash:bybit:main:USDT"


def test_balances_profile_tokenmm_filters_unrelated_shared_account_assets(
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
                "exchange": "bybit",
                "account": "main",
                "asset": "PLUME",
                "free": "10",
                "total": "10",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "account": "main",
                "asset": "USDT",
                "free": "100",
                "total": "100",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "account": "main",
                "asset": "ZENT",
                "free": "500",
                "total": "500",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "signed_qty": "3",
                "quantity": "3",
                "side": "LONG",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "BTCUSDT-LINEAR.BYBIT",
                "signed_qty": "1",
                "quantity": "1",
                "side": "LONG",
                "ts_ms": 2_000,
            },
        ],
    )
    redis_client.set_json(
        primary_keys.market_last(exchange="bybit", base="PLUME", quote="USDT"),
        {"bid": 0.01, "ask": 0.011, "ts_ms": 2_000},
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(ContractCatalogEntry(exchange="bybit", symbol="PLUME/USDT"),),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row_ids = {row["row_id"] for row in body["data"]["rows"]}
    assert "tokenmm:cash:bybit:main:PLUME" in row_ids
    assert "tokenmm:cash:bybit:main:USDT" in row_ids
    assert "tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT" in row_ids
    assert "tokenmm:cash:bybit:main:ZENT" not in row_ids
    assert "tokenmm:pos:bybit:BTCUSDT-LINEAR.BYBIT" not in row_ids


def test_balances_profile_tokenmm_prefers_cash_row_over_duplicate_spot_position(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "account": "main",
                "asset": "PLUME",
                "free": "-59110.34159317",
                "total": "-59110.34159317",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "signed_qty": "-228161.2",
                "quantity": "228161.2",
                "side": "SHORT",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "signed_qty": "84919",
                "quantity": "84919",
                "side": "LONG",
                "ts_ms": 2_000,
            },
        ],
    )
    redis_client.set_json(
        keys.market_last(exchange="bybit", base="PLUME", quote="USDT"),
        {"bid": 0.0110, "ask": 0.0111, "ts_ms": 2_000},
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-SPOT.BYBIT",
            ),
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-LINEAR.BYBIT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row_ids = {row["row_id"] for row in body["data"]["rows"]}
    assert "tokenmm:cash:bybit:main:PLUME" in row_ids
    assert "tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT" in row_ids
    assert "tokenmm:pos:bybit:PLUMEUSDT-SPOT.BYBIT" not in row_ids


def test_balances_profile_tokenmm_keeps_duplicate_spot_position_when_cash_row_is_other_account(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "account": "acctA",
                "asset": "PLUME",
                "free": "-59110.34159317",
                "total": "-59110.34159317",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "account": "acctB",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "signed_qty": "-228161.2",
                "quantity": "228161.2",
                "side": "SHORT",
                "ts_ms": 2_000,
            },
        ],
    )
    redis_client.set_json(
        keys.market_last(exchange="bybit", base="PLUME", quote="USDT"),
        {"bid": 0.0110, "ask": 0.0111, "ts_ms": 2_000},
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-SPOT.BYBIT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row_ids = {row["row_id"] for row in body["data"]["rows"]}
    assert "tokenmm:cash:bybit:acctA:PLUME" in row_ids
    assert "tokenmm:pos:bybit:PLUMEUSDT-SPOT.BYBIT" in row_ids


def test_balances_mark_cash_assets_and_positions_from_market_data(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "account": "main",
                "asset": "ABC",
                "free": "10",
                "total": "10",
                "row_id": "cash-abc",
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "kind": "position",
                "instrument_id": "ABCUSDT-PERP.VENUE_A",
                "signed_qty": "-2",
                "quantity": "2",
                "side": "SHORT",
                "avg_px_open": "95",
                "row_id": "pos-abc",
            },
        ],
    )
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {
            "bid": 99.0,
            "ask": 101.0,
            "ts_ms": 1_700_000_000_000,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances")
        body = response.get_json()

    assert response.status_code == 200
    rows = {row["row_id"]: row for row in body["data"]["rows"]}
    assert rows["cash-abc"]["mark_raw"] == pytest.approx(100.0)
    assert rows["cash-abc"]["mv_raw"] == pytest.approx(1_000.0)
    assert (
        rows[f"{flux_config.identity.strategy_id}:pos:{'venue_a'}:ABCUSDT-PERP.VENUE_A"]["asset"]
        == "ABC"
    )
    assert rows[f"{flux_config.identity.strategy_id}:pos:{'venue_a'}:ABCUSDT-PERP.VENUE_A"][
        "mark_raw"
    ] == pytest.approx(95.0)
    assert rows[f"{flux_config.identity.strategy_id}:pos:{'venue_a'}:ABCUSDT-PERP.VENUE_A"][
        "mv_raw"
    ] == pytest.approx(-190.0)


def test_instrument_catalog_reads_legacy_market_keys_for_signals_and_balances(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    market_ts_ms = 1_700_000_000_000
    redis_client.set_json(
        keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "signed_qty": "3",
                "quantity": "3",
                "side": "LONG",
                "ts_ms": 2_000,
            },
        ],
    )
    redis_client.set_json(
        keys.market_last(exchange="bybit", base="PLUME", quote="USDT"),
        {"bid": 0.01, "ask": 0.011, "ts_ms": market_ts_ms},
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-LINEAR.BYBIT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        signals_response = client.get("/api/v1/signals")
        signals_body = signals_response.get_json()
        balances_response = client.get("/api/v1/balances")
        balances_body = balances_response.get_json()

    assert signals_response.status_code == 200
    leg = signals_body["data"]["strategies"][0]["legs"]["bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert leg["bid"] == pytest.approx(0.01)
    assert leg["ask"] == pytest.approx(0.011)
    assert leg["mid"] == pytest.approx(0.0105)
    assert leg["ts_ms"] == market_ts_ms

    assert balances_response.status_code == 200
    position = balances_body["data"]["rows"][0]
    assert position["mark_raw"] == pytest.approx(0.0105)
    assert position["mv_raw"] == pytest.approx(0.0315)


def test_balances_with_strategy_query_keeps_per_strategy_debug_view(
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
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "bybit",
                "account": "main",
                "asset": "USDT",
                "free": "50",
                "total": "50",
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
        profile_required_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get(
            "/api/v1/balances",
            query_string={"profile": "tokenmm", "strategy": "strategy_02"},
        )
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    assert len(rows) == 1
    assert rows[0]["strategy_id"] == "strategy_02"
    assert rows[0].get("row_id") != "tokenmm:cash:bybit:main:USDT"


def test_balances_profile_tokenmm_marks_missing_required_components_as_degraded(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 100_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    missing_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(missing_keys.params_hash_key(), {"qty": "2.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "asset": "USDT",
                "free": "10",
                "total": "10",
                "ts_ms": 95_000,
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
        profile_required_strategy_map={"tokenmm": ["strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is True
    assert body["data"]["missing_required"] == ["strategy_02"]
    components = {row["strategy_id"]: row for row in body["data"]["components"]}
    assert components["strategy_02"]["snapshot_present"] is False
    assert components["strategy_02"]["missing"] is True
    assert components["strategy_02"]["stale"] is True


def test_balances_profile_tokenmm_prefers_canonical_portfolio_snapshot_when_present(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 123_456)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "bybit",
                "asset": "USDT",
                "free": "999",
                "total": "999",
                "mv_raw": 999.0,
                "ts_ms": 123_000,
            },
        ],
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "ABC",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "ABC",
                "global_qty_base": "129016.69578451",
                "global_qty": "129016.69578451",
                "aggregation_mode": "partial",
                "global_qty_base_complete": False,
                "global_qty_complete": False,
                "usable_component_count": 4,
                "expected_component_count": 7,
                "missing_required": ["strategy_02"],
                "stale_required": ["strategy_03"],
                "null_qty_required": ["strategy_04"],
                "degraded": True,
                "ts_ms": 122_000,
                "stale_after_ms": 3_000,
                "components": [
                    {
                        "strategy_id": flux_config.identity.strategy_id,
                        "local_qty_base": "15.000000",
                        "local_qty": "15.000000",
                        "ts_ms": 122_000,
                        "state": "running",
                    },
                    {
                        "strategy_id": "strategy_02",
                        "local_qty_base": None,
                        "local_qty": None,
                        "ts_ms": 122_000,
                        "state": "running",
                    },
                ],
            },
            "components": [
                {
                    "strategy_id": flux_config.identity.strategy_id,
                    "local_qty_base": "15.000000",
                    "local_qty": "15.000000",
                    "ts_ms": 122_000,
                    "state": "running",
                },
                {
                    "strategy_id": "strategy_02",
                    "local_qty_base": None,
                    "local_qty": None,
                    "ts_ms": 122_000,
                    "state": "running",
                },
            ],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:bybit:ABC",
                        "strategy_id": "tokenmm",
                        "exchange": "bybit",
                        "asset": "ABC",
                        "free": "15",
                        "total": "15",
                        "mv_raw": 21.0,
                        "mark_raw": 1.4,
                        "ts_ms": 122_000,
                    },
                ],
                "totals": {
                    "mv_raw": 21.0,
                    "mv_display": "$21.00",
                },
            },
            "server_ts_ms": 122_500,
        },
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot"
    assert body["data"]["server_ts_ms"] == 122_500
    assert body["data"]["global_qty_base"] == "129016.69578451"
    assert body["data"]["global_qty"] == "129016.69578451"
    assert body["data"]["aggregation_mode"] == "partial"
    assert body["data"]["global_qty_base_complete"] is False
    assert body["data"]["global_qty_complete"] is False
    assert body["data"]["missing_required"] == ["strategy_02"]
    assert body["data"]["stale_required"] == ["strategy_03"]
    assert body["data"]["null_qty_required"] == ["strategy_04"]
    assert body["data"]["rows"][0]["total"] == "15"
    assert body["data"]["totals"]["mv_raw"] == 21.0
    assert body["data"]["components"] == [
        {
            "strategy_id": flux_config.identity.strategy_id,
            "local_qty_base": "15.000000",
            "local_qty": "15.000000",
            "ts_ms": 122_000,
            "state": "running",
        },
        {
            "strategy_id": "strategy_02",
            "local_qty_base": None,
            "local_qty": None,
            "ts_ms": 122_000,
            "state": "running",
        },
    ]


def test_balances_profile_tokenmm_emits_backend_risk_groups_and_row_annotations(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 123_456)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "account": "ACC-1",
                "asset": "USDT",
                "free": "10",
                "total": "10",
                "mark_raw": 1.0,
                "mv_raw": 10.0,
                "ts_ms": 123_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "account": "ACC-1",
                "asset": "ABC",
                "free": "5",
                "total": "5",
                "mark_raw": 2.0,
                "mv_raw": 10.0,
                "ts_ms": 123_000,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "venue_b",
                "account": "ACC-2",
                "asset": "USDT",
                "free": "7",
                "total": "7",
                "mark_raw": 1.0,
                "mv_raw": 7.0,
                "ts_ms": 123_100,
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    by_row_id = {row["row_id"]: row for row in body["data"]["rows"]}
    assert by_row_id["tokenmm:cash:venue_a:ACC-1:USDT"]["risk_key"] == "USD_CASH"
    assert by_row_id["tokenmm:cash:venue_a:ACC-1:USDT"]["risk_label"] == "USD Cash"
    assert by_row_id["tokenmm:cash:venue_b:ACC-2:USDT"]["risk_key"] == "USD_CASH"
    abc_row = next(row for row in body["data"]["rows"] if row["asset"] == "ABC")
    assert abc_row["risk_key"] == "ABC"

    risk_groups = {group["risk_key"]: group for group in body["data"]["risk_groups"]}
    assert risk_groups["USD_CASH"]["label"] == "USD Cash"
    assert risk_groups["USD_CASH"]["net_mv"] == pytest.approx(17.0)
    assert risk_groups["USD_CASH"]["gross_mv"] == pytest.approx(17.0)
    assert set(risk_groups["USD_CASH"]["sources"]) == {"venue_a", "venue_b"}
    assert len(risk_groups["USD_CASH"]["rows"]) == 2
    assert {row["coin"] for row in risk_groups["USD_CASH"]["rows"]} == {"USDT"}
    assert risk_groups["ABC"]["gross_mv"] == pytest.approx(10.0)
    assert [row["coin"] for row in risk_groups["ABC"]["rows"]] == ["ABC"]


@pytest.mark.parametrize(
    ("server_ts_ms", "inventory_ts_ms"),
    [
        (119_000, 122_500),
        (122_500, 119_000),
    ],
)
def test_balances_profile_tokenmm_rejects_stale_portfolio_snapshot_and_falls_back_to_live_balances(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
    server_ts_ms,
    inventory_ts_ms,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 123_456)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "exchange": "venue_a",
                "asset": "ABC",
                "free": "5",
                "total": "5",
                "mark_raw": 1.5,
                "mv_raw": 7.5,
                "ts_ms": 123_000,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "venue_a",
                "asset": "USDT",
                "free": "12",
                "total": "12",
                "ts_ms": 123_100,
            },
        ],
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "ABC",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "ABC",
                "global_qty_base": "999",
                "global_qty": "999",
                "aggregation_mode": "strict",
                "global_qty_base_complete": True,
                "global_qty_complete": True,
                "ts_ms": inventory_ts_ms,
                "stale_after_ms": 3_000,
                "components": [],
            },
            "components": [],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:venue_a:ABC",
                        "strategy_id": "tokenmm",
                        "exchange": "venue_a",
                        "asset": "ABC",
                        "free": "999",
                        "total": "999",
                        "mark_raw": 1.0,
                        "mv_raw": 999.0,
                        "ts_ms": 122_500,
                    },
                ],
                "totals": {
                    "mv_raw": 999.0,
                    "mv_display": "$999.00",
                },
            },
            "server_ts_ms": server_ts_ms,
        },
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"].get("source") is None
    assert body["data"]["server_ts_ms"] == 123_456
    by_row_id = {row["row_id"]: row for row in body["data"]["rows"]}
    assert by_row_id["tokenmm:cash:venue_a::ABC"]["total"] == "5"
    assert by_row_id["tokenmm:cash:venue_a::ABC"]["mv_raw"] == pytest.approx(7.5)
    assert by_row_id["tokenmm:cash:venue_a::USDT"]["total"] == "12"
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(19.5)


def test_balances_profile_tokenmm_portfolio_snapshot_uses_market_rows_from_all_profile_strategies(
    monkeypatch,
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 100_000)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        secondary_keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {
            "bid": 1.5,
            "ask": 2.5,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "ABC",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "ABC",
                "global_qty_base": "5",
                "global_qty": "5",
                "aggregation_mode": "strict",
                "global_qty_base_complete": True,
                "global_qty_complete": True,
                "ts_ms": 99_000,
                "stale_after_ms": 3_000,
                "components": [],
            },
            "components": [],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:venue_a:ABC",
                        "strategy_id": "tokenmm",
                        "exchange": "venue_a",
                        "asset": "ABC",
                        "free": "5",
                        "total": "5",
                        "ts_ms": 99_000,
                    },
                ],
                "totals": {
                    "mv_raw": 0.0,
                    "mv_display": "$0.00",
                },
            },
            "server_ts_ms": 99_500,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot"
    assert body["data"]["rows"][0]["mark_raw"] == pytest.approx(2.0)
    assert body["data"]["rows"][0]["mv_raw"] == pytest.approx(10.0)
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(10.0)


def test_balances_profile_tokenmm_portfolio_snapshot_preserves_non_null_market_quotes_across_strategies(
    monkeypatch,
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_001_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        primary_keys.market_last(
            exchange="bybit",
            base="PLUME",
            quote="USDT",
            instrument_id="PLUMEUSDT-SPOT.BYBIT",
        ),
        {
            "bid": 0.0112,
            "ask": 0.0113,
            "ts_ms": 1_700_000_000_000,
        },
    )
    redis_client.set_json(
        secondary_keys.market_last(
            exchange="bybit",
            base="PLUME",
            quote="USDT",
            instrument_id="PLUMEUSDT-SPOT.BYBIT",
        ),
        {
            "bid": None,
            "ask": None,
            "ts_ms": 1_700_000_000_100,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "USDT",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "USDT",
                "global_qty_base": "0",
                "global_qty": "0",
                "aggregation_mode": "strict",
                "global_qty_base_complete": True,
                "global_qty_complete": True,
                "ts_ms": 1_700_000_000_900,
                "stale_after_ms": 3_000,
                "components": [],
            },
            "components": [],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:bybit:BYBIT-UNIFIED:spot:PLUME",
                        "strategy_id": "tokenmm",
                        "exchange": "bybit",
                        "account_id": "BYBIT-UNIFIED",
                        "account": "BYBIT-UNIFIED",
                        "asset": "PLUME",
                        "coin": "PLUME",
                        "base": "PLUME",
                        "free": "-62331.89500742",
                        "locked": "0.00000000",
                        "total": "-62331.89500742",
                        "product_type": "spot",
                        "market_type": "spot",
                        "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                        "ts_ms": 1_700_000_000_000,
                    },
                ],
                "totals": {
                    "mv_raw": 0.0,
                    "mv_display": "$0.00",
                },
            },
            "server_ts_ms": 1_700_000_000_500,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-LINEAR.BYBIT",
            ),
            ContractCatalogEntry(
                exchange="bybit",
                symbol="PLUME/USDT",
                instrument_id="PLUMEUSDT-SPOT.BYBIT",
            ),
        ),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot"
    assert body["data"]["rows"][0]["row_id"] == "tokenmm:cash:bybit:BYBIT-UNIFIED:spot:PLUME"
    assert body["data"]["rows"][0]["mark_raw"] == pytest.approx(0.01125)
    assert body["data"]["rows"][0]["mv_raw"] == pytest.approx(-701.233818833475)
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(-701.233818833475)


def test_balances_profile_tokenmm_portfolio_snapshot_prefers_newest_non_null_market_quote_across_strategies(
    monkeypatch,
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 1_700_000_001_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="strategy_02",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_json(
        primary_keys.market_last(
            exchange="venue_a",
            base="ABC",
            quote="USDT",
        ),
        {
            "bid": 1.0,
            "ask": 3.0,
            "ts_ms": 1_700_000_000_900,
        },
    )
    redis_client.set_json(
        secondary_keys.market_last(
            exchange="venue_a",
            base="ABC",
            quote="USDT",
        ),
        {
            "bid": 9.0,
            "ask": 11.0,
            "ts_ms": 1_700_000_000_100,
        },
    )
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "USDT",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "USDT",
                "global_qty_base": "0",
                "global_qty": "0",
                "aggregation_mode": "strict",
                "global_qty_base_complete": True,
                "global_qty_complete": True,
                "ts_ms": 1_700_000_000_900,
                "stale_after_ms": 3_000,
                "components": [],
            },
            "components": [],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:venue_a:main:ABC",
                        "strategy_id": "tokenmm",
                        "exchange": "venue_a",
                        "account": "main",
                        "asset": "ABC",
                        "free": "5",
                        "total": "5",
                        "ts_ms": 1_700_000_000_800,
                    },
                ],
                "totals": {
                    "mv_raw": 0.0,
                    "mv_display": "$0.00",
                },
            },
            "server_ts_ms": 1_700_000_000_950,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=(ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),),
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "strategy_02"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot"
    assert body["data"]["rows"][0]["mark_raw"] == pytest.approx(2.0)
    assert body["data"]["rows"][0]["mv_raw"] == pytest.approx(10.0)
    assert body["data"]["totals"]["mv_raw"] == pytest.approx(10.0)


def test_balances_profile_tokenmm_staleness_clears_when_all_components_fresh(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 100_000)
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
                "exchange": "bybit",
                "asset": "USDT",
                "free": "10",
                "total": "10",
                "ts_ms": 95_000,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "strategy_02",
                "exchange": "bybit",
                "asset": "USDT",
                "free": "20",
                "total": "20",
                "ts_ms": 98_000,
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is False
    assert body["data"]["missing_required"] == []
    assert all(component["stale"] is False for component in body["data"]["components"])


def test_balances_profile_tokenmm_canonicalizes_bitget_shared_account_cash(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="plumeusdt_bitget_perp_makerv3",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "account_id": "BITGET-001",
                "exchange": "bitget",
                "asset": "USDT",
                "free": "500",
                "locked": "0",
                "total": "500",
                "product_type": "spot",
                "ts_ms": 1_700_000_000_000,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "plumeusdt_bitget_perp_makerv3",
                "account_id": "BITGET-001",
                "exchange": "bitget",
                "asset": "USDT",
                "free": "0",
                "locked": "0",
                "total": "0",
                "product_type": "perp",
                "ts_ms": 1_700_000_000_100,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "plumeusdt_bitget_perp_makerv3"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    bitget_rows = sorted(
        [
            row
            for row in body["data"]["rows"]
            if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
        ],
        key=lambda row: str(row["row_id"]),
    )
    assert len(bitget_rows) == 1
    row = bitget_rows[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["total"] == "500"
    assert row["product_type"] == "spot"
    assert row["display_name_short"] == "USDT"
    assert row.get("scope") == "shared_account"


def test_balances_profile_tokenmm_canonicalizes_bitget_shared_account_cash_alongside_non_stable_rows(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    secondary_keys = FluxRedisKeys(
        strategy_id="plumeusdt_bitget_perp_makerv3",
        namespace=flux_config.identity.namespace,
        schema_version=flux_config.identity.schema_version,
    )
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_hash_json(secondary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "account_id": "BITGET-001",
                "exchange": "bitget",
                "asset": "USDT",
                "free": "500",
                "locked": "0",
                "total": "500",
                "product_type": "spot",
                "ts_ms": 1_700_000_000_000,
            },
            {
                "strategy_id": flux_config.identity.strategy_id,
                "account_id": "BITGET-001",
                "exchange": "bitget",
                "asset": "PLUME",
                "free": "25000",
                "locked": "0",
                "total": "25000",
                "product_type": "spot",
                "ts_ms": 1_700_000_000_010,
            },
        ],
    )
    redis_client.set_json(
        secondary_keys.balances_snapshot(),
        [
            {
                "strategy_id": "plumeusdt_bitget_perp_makerv3",
                "account_id": "BITGET-001",
                "exchange": "bitget",
                "asset": "USDT",
                "free": "0",
                "locked": "0",
                "total": "0",
                "product_type": "perp",
                "ts_ms": 1_700_000_000_100,
            },
        ],
    )

    bitget_contract_catalog = (
        ContractCatalogEntry(
            exchange="bitget",
            symbol="PLUME/USDT",
            instrument_id="PLUMEUSDT.BITGET",
        ),
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=bitget_contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={
            "tokenmm": [flux_config.identity.strategy_id, "plumeusdt_bitget_perp_makerv3"],
        },
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    bitget_rows = sorted(
        [
            row
            for row in body["data"]["rows"]
            if row.get("exchange") == "bitget" and row.get("account") == "BITGET-001"
        ],
        key=lambda row: str(row["row_id"]),
    )

    assert [row["row_id"] for row in bitget_rows] == [
        "tokenmm:cash:bitget:BITGET-001:PLUME",
        "tokenmm:cash:bitget:BITGET-001:USDT",
    ]
    assert [row.get("product_type") for row in bitget_rows] == ["spot", "spot"]
    assert [row["total"] for row in bitget_rows] == ["25000", "500"]
    assert [row.get("scope") for row in bitget_rows] == [None, "shared_account"]


def test_balances_profile_tokenmm_portfolio_snapshot_canonicalizes_shared_stable_cash_without_changing_global_qty(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    monkeypatch.setattr(app_module, "now_ms", lambda: 123_456)
    redis_client.set_json(
        FluxRedisKeys.portfolio_snapshot(
            portfolio_id="tokenmm",
            namespace=flux_config.identity.namespace,
            schema_version=flux_config.identity.schema_version,
        ),
        {
            "portfolio_id": "tokenmm",
            "base_currency": "PLUME",
            "inventory": {
                "portfolio_id": "tokenmm",
                "base_currency": "PLUME",
                "global_qty_base": "79577.70469832",
                "global_qty": "79577.70469832",
                "aggregation_mode": "partial",
                "global_qty_base_complete": True,
                "global_qty_complete": True,
                "missing_required": [],
                "stale_required": [],
                "null_qty_required": [],
                "degraded": False,
                "ts_ms": 122_000,
                "stale_after_ms": 3_000,
                "components": [],
            },
            "components": [],
            "balances": {
                "rows": [
                    {
                        "row_id": "tokenmm:cash:bitget:BITGET-001:spot:USDT",
                        "strategy_id": "tokenmm",
                        "exchange": "bitget",
                        "account": "BITGET-001",
                        "account_id": "BITGET-001",
                        "asset": "USDT",
                        "free": "440.735561",
                        "total": "440.735561",
                        "product_type": "spot",
                        "scope": "shared_account",
                        "mark_raw": 1.0,
                        "mv_raw": 440.735561,
                        "ts_ms": 122_000,
                    },
                    {
                        "row_id": "tokenmm:cash:bitget:BITGET-001:perp:USDT",
                        "strategy_id": "tokenmm",
                        "exchange": "bitget",
                        "account": "BITGET-001",
                        "account_id": "BITGET-001",
                        "asset": "USDT",
                        "free": "440.735561",
                        "total": "440.735561",
                        "product_type": "perp",
                        "scope": "shared_account",
                        "mark_raw": 1.0,
                        "mv_raw": 440.735561,
                        "ts_ms": 122_100,
                    },
                ],
                "totals": {
                    "mv_raw": 881.471122,
                    "mv_display": "$881.47",
                },
            },
            "server_ts_ms": 122_500,
        },
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["source"] == "portfolio_snapshot"
    assert body["data"]["global_qty_base"] == "79577.70469832"
    bitget_rows = [
        row
        for row in body["data"]["rows"]
        if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
    ]
    assert len(bitget_rows) == 1
    assert bitget_rows[0]["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert bitget_rows[0]["total"] == "440.735561"
    assert bitget_rows[0]["product_type"] == "spot"
    assert bitget_rows[0]["display_name_short"] == "USDT"
    assert bitget_rows[0]["display_name_long"] == "Bitget USDT"
    risk_groups = {group["risk_key"]: group for group in body["data"]["risk_groups"]}
    assert risk_groups["USD_CASH"]["net_mv"] == pytest.approx(440.735561)
    assert risk_groups["USD_CASH"]["gross_mv"] == pytest.approx(440.735561)


def test_balances_profile_tokenmm_uses_event_balance_timestamps_for_freshness(
    monkeypatch,
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    event_ts_ms = 1_700_000_098_000
    monkeypatch.setattr(app_module, "now_ms", lambda: event_ts_ms + 2_000)
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_hash_json(primary_keys.params_hash_key(), {"qty": "1.0"})
    redis_client.set_json(
        primary_keys.balances_snapshot(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "events": [
                    {
                        "account_id": "BYBIT-main",
                        "ts_ms": event_ts_ms,
                        "balances": [
                            {
                                "currency": "USDT",
                                "free": "10",
                                "locked": "0",
                                "total": "10",
                            },
                        ],
                    },
                ],
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

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["degraded"] is False
    assert body["data"]["missing_required"] == []
    component = body["data"]["components"][0]
    assert component["latest_ts_ms"] == event_ts_ms
    assert component["stale"] is False
    assert body["data"]["rows"][0]["ts_ms"] == event_ts_ms


def test_params_profile_tokenmm_does_not_discover_unallowlisted_strategies(
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

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["strategy_id"] for row in body["data"]] == [flux_config.identity.strategy_id]


def test_alerts_profile_tokenmm_aggregates_allowlisted_strategies(
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
    redis_client.add_stream_rows(
        primary_keys.alerts(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "row_id": "strategy_01:alert:1",
                "level": "INFO",
                "message": "primary",
                "ts_ms": 1_000,
            },
        ],
    )
    redis_client.add_stream_rows(
        secondary_keys.alerts(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "strategy_02:alert:1",
                "level": "CRITICAL",
                "message": "secondary",
                "ts_ms": 2_000,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/alerts", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["total"] == 2
    rows = body["data"]["rows"]
    assert [row["strategy_id"] for row in rows] == ["strategy_02", flux_config.identity.strategy_id]


def test_alerts_profile_tokenmm_stabilizes_row_id_from_entry_id(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    primary_keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.add_stream_rows(
        primary_keys.alerts(),
        [
            {
                "strategy_id": flux_config.identity.strategy_id,
                "level": "ERROR",
                "message": "borrow denied",
                "ts_ms": 1_000,
            },
        ],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/alerts", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    row = body["data"]["rows"][0]
    assert row["entry_id"]
    assert row["row_id"] == row["entry_id"]


def test_alerts_delete_profile_tokenmm_clears_all_allowlisted_strategies(
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
    redis_client.add_stream_rows(
        primary_keys.alerts(),
        [{"strategy_id": flux_config.identity.strategy_id, "row_id": "strategy_01:alert:1"}],
    )
    redis_client.add_stream_rows(
        secondary_keys.alerts(),
        [{"strategy_id": "strategy_02", "row_id": "strategy_02:alert:1"}],
    )

    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id, "strategy_02"]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.delete("/api/v1/alerts", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["deleted"] == 2
    assert body["data"]["remaining"] == 0
    assert body["data"]["profile"] == "tokenmm"
    assert body["data"]["strategy_ids"] == [flux_config.identity.strategy_id, "strategy_02"]
    assert body["data"]["remaining_by_strategy"] == {
        flux_config.identity.strategy_id: 0,
        "strategy_02": 0,
    }


def test_strategy_parameters_update_rejects_unallowlisted_tokenmm_strategy_path(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.patch(
            "/api/v1/strategies/strategy_02/parameters",
            json={"updates": {"qty": 2}},
        )
        body = response.get_json()

    assert response.status_code == 404
    assert body["ok"] is False
    assert body["error"]["code"] == "unknown_strategy_id"


def test_alerts_delete_rejects_unallowlisted_tokenmm_strategy_query(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    app = create_flux_api_app(
        flux_config,
        redis_client,
        contract_catalog=contract_catalog,
        strategy_metadata=strategy_metadata,
        profile_strategy_map={"tokenmm": [flux_config.identity.strategy_id]},
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.delete("/api/v1/alerts", query_string={"strategy": "strategy_02"})
        body = response.get_json()

    assert response.status_code == 404
    assert body["ok"] is False
    assert body["error"]["code"] == "unknown_strategy_id"
