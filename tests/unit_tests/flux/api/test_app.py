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

import nautilus_trader.flux.api.app as app_module
from nautilus_trader.flux.api import DEFAULT_PARAMS_DEFAULTS
from nautilus_trader.flux.api import DEFAULT_PARAMS_SCHEMA
from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api.payloads import StrategyMetadata
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
                "exchange": "bybit",
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


def test_signals_profile_prefers_discovered_tokenmm_strategy_over_default_mapping(
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
    assert body["data"]["strategies"][0]["id"] == "strategy_02"


def test_trades_endpoint_uses_profile_strategy_resolution_when_strategy_query_is_omitted(
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
    redis_client.add_stream_rows(
        alt_keys.trades_stream(),
        [
            {
                "strategy_id": "strategy_02",
                "row_id": "t-1",
                "seq": 1,
                "ts_ms": 1_000,
                "coin": "PLUME",
                "exchange": "bybit",
                "side": "buy",
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
        response = client.get("/api/v1/trades", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    assert [row["row_id"] for row in body["data"]["rows"]] == ["t-1"]


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

    assert sort_asc_response.status_code == 200
    assert [row["row_id"] for row in sort_asc_body["data"]["rows"]] == ["t-1", "t-2", "t-3"]
    assert sort_asc_body["data"]["sort"] == "ts_ms_asc"

    assert sort_desc_response.status_code == 200
    assert [row["row_id"] for row in sort_desc_body["data"]["rows"]] == ["t-3", "t-2", "t-1"]
    assert sort_desc_body["data"]["sort"] == "ts_ms_desc"


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
                "exchange": "bybit",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
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
                "exchange": "bybit",
                "account": "main",
                "asset": "USDT",
                "free": "140",
                "total": "140",
                "ts_ms": 2_000,
            },
            {
                "strategy_id": "strategy_02",
                "kind": "position",
                "exchange": "bybit",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
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
        params_schema=params_schema,
        params_defaults=params_defaults,
    )

    with app.test_client() as client:
        response = client.get("/api/v1/balances", query_string={"profile": "tokenmm"})
        body = response.get_json()

    assert response.status_code == 200
    rows = body["data"]["rows"]
    by_row_id = {row["row_id"]: row for row in rows}
    cash_row = by_row_id["tokenmm:cash:bybit:main:USDT"]
    assert cash_row["free"] == "140"
    assert cash_row["total"] == "140"
    assert cash_row["strategy_id"] == "tokenmm"

    position_row = by_row_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert position_row["signed_qty"] == "1"
    assert position_row["quantity"] == "1"
    assert position_row["side"] == "LONG"
    assert position_row["strategy_id"] == "tokenmm"


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
