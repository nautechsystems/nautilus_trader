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

import pytest

from nautilus_trader.flux.api import create_flux_api_app
from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.common.keys import FluxRedisKeys


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
            contract_catalog=(
                ContractCatalogEntry(exchange="venue_a", symbol="INVALID"),
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
