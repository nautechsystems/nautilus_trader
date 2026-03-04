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

from nautilus_trader.flux.api import ContractCatalogEntry
from nautilus_trader.flux.api import create_flux_api_app
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


def test_signals_legs_key_by_contract_id_for_multiple_contracts_on_same_exchange(
    flux_config,
    redis_client,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    _seed_required_schema_keys(redis_client, flux_config)
    contract_catalog = (
        ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),
        ContractCatalogEntry(exchange="venue_a", symbol="XYZ/USDT"),
    )
    keys = FluxRedisKeys.from_identity(flux_config.identity)
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="ABC", quote="USDT"),
        {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000100},
    )
    redis_client.set_json(
        keys.market_last(exchange="venue_a", base="XYZ", quote="USDT"),
        {"bid": 200.0, "ask": 201.0, "ts_ms": 1700000000200},
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
    strategy = body["data"]["strategies"][0]
    assert strategy["legs_order"] == ["venue_a:ABC/USDT", "venue_a:XYZ/USDT"]
    assert set(strategy["legs"].keys()) == {"venue_a:ABC/USDT", "venue_a:XYZ/USDT"}
    assert strategy["legs"]["venue_a:ABC/USDT"]["symbol"] == "ABC/USDT"
    assert strategy["legs"]["venue_a:ABC/USDT"]["bid"] == 100.0
    assert strategy["legs"]["venue_a:XYZ/USDT"]["symbol"] == "XYZ/USDT"
    assert strategy["legs"]["venue_a:XYZ/USDT"]["bid"] == 200.0


def test_param_schema_and_params_get_shapes_are_tokenmm_compatible(
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
        schema_response = client.get("/api/v1/param-schema", query_string={"profile": "tokenmm"})
        schema_body = schema_response.get_json()
        params_response = client.get("/api/v1/params", query_string={"profile": "tokenmm"})
        params_body = params_response.get_json()

    assert schema_response.status_code == 200
    assert set(schema_body["data"].keys()) == {"params", "deprecated"}
    assert schema_body["data"]["deprecated"] == {}
    assert "qty" in schema_body["data"]["params"]

    assert params_response.status_code == 200
    assert isinstance(params_body["data"], list)
    assert len(params_body["data"]) == 1
    row = params_body["data"][0]
    assert row["strategy_id"] == flux_config.identity.strategy_id
    assert row["params"]["qty"] == 1.0


def test_patch_params_supports_bulk_and_legacy_payloads(
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

    bulk_payload = {
        "updates": [
            {"strategy_id": flux_config.identity.strategy_id, "params": {"qty": 2.5}},
            {"strategy_id": "strategy_02", "params": {"qty": 3.5}},
        ],
        "source": "tokenmm-ui",
    }
    legacy_payload = {"params": {"qty": 4.5}, "source": "legacy"}

    with app.test_client() as client:
        bulk_response = client.patch("/api/v1/params", json=bulk_payload)
        bulk_body = bulk_response.get_json()
        legacy_response = client.patch(
            "/api/v1/params",
            query_string={"strategy": flux_config.identity.strategy_id},
            json=legacy_payload,
        )
        legacy_body = legacy_response.get_json()
        params_response = client.get("/api/v1/params", query_string={"strategy": "strategy_02"})
        params_body = params_response.get_json()

    assert bulk_response.status_code == 200
    assert set(bulk_body["data"].keys()) == {"success", "failed", "errors"}
    assert len(bulk_body["data"]["success"]) == 2
    assert bulk_body["data"]["failed"] == []
    assert bulk_body["data"]["errors"] == []

    assert legacy_response.status_code == 200
    assert set(legacy_body["data"].keys()) == {"success", "failed", "errors"}
    assert len(legacy_body["data"]["success"]) == 1
    assert legacy_body["data"]["failed"] == []
    assert legacy_body["data"]["errors"] == []

    assert params_response.status_code == 200
    assert params_body["data"][0]["strategy_id"] == "strategy_02"
    assert params_body["data"][0]["params"]["qty"] == 3.5


def test_patch_params_bulk_item_requires_non_empty_strategy_id(
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

    payload = {
        "updates": [
            {"params": {"qty": 9.0}},
            {"strategy_id": " ", "params": {"qty": 8.0}},
            {"strategy_id": "strategy_02", "params": {"qty": 7.0}},
        ],
        "source": "tokenmm-ui",
    }

    with app.test_client() as client:
        response = client.patch("/api/v1/params", json=payload)
        body = response.get_json()
        default_params_response = client.get("/api/v1/params")
        default_params_body = default_params_response.get_json()
        strategy_02_response = client.get("/api/v1/params", query_string={"strategy": "strategy_02"})
        strategy_02_body = strategy_02_response.get_json()

    assert response.status_code == 200
    assert len(body["data"]["success"]) == 1
    assert body["data"]["success"][0]["strategy_id"] == "strategy_02"
    assert set(body["data"]["failed"]) == {""}
    assert len(body["data"]["errors"]) == 2
    assert all(error["code"] == "invalid_strategy_id" for error in body["data"]["errors"])

    assert default_params_response.status_code == 200
    assert default_params_body["data"][0]["params"]["qty"] == 1.0
    assert strategy_02_response.status_code == 200
    assert strategy_02_body["data"][0]["params"]["qty"] == 7.0


def test_trades_pagination_and_delta_shapes_match_tokenmm_contract(
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
            {"strategy_id": flux_config.identity.strategy_id, "seq": 101, "ts_ms": 101_000},
            {"strategy_id": flux_config.identity.strategy_id, "row_id": "t-102", "seq": 102, "ts_ms": 102_000},
            {"strategy_id": flux_config.identity.strategy_id, "row_id": "t-103", "seq": 103, "ts_ms": 103_000},
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
        trades_response = client.get("/api/v1/trades", query_string={"limit": 2, "offset": 0})
        trades_body = trades_response.get_json()
        delta_response = client.get("/api/v1/trades/delta", query_string={"since_seq": 100, "limit": 2})
        delta_body = delta_response.get_json()

    assert trades_response.status_code == 200
    assert set(trades_body["data"].keys()) >= {"rows", "total", "limit", "offset", "has_more"}
    assert trades_body["data"]["total"] == 3
    assert trades_body["data"]["limit"] == 2
    assert trades_body["data"]["offset"] == 0
    assert trades_body["data"]["has_more"] is True
    assert len(trades_body["data"]["rows"]) == 2
    for row in trades_body["data"]["rows"]:
        assert isinstance(row.get("row_id"), str) and row["row_id"]
        assert isinstance(row.get("ts_ms"), int)
        assert isinstance(row.get("version"), int)

    assert delta_response.status_code == 200
    assert set(delta_body["data"].keys()) >= {"rows", "last_seq", "reset_required"}
    assert delta_body["data"]["reset_required"] is False
    assert delta_body["data"]["last_seq"] == 102
    assert [row["seq"] for row in delta_body["data"]["rows"]] == [101, 102]
    generated_row = delta_body["data"]["rows"][0]
    assert generated_row["row_id"].startswith(f"{flux_config.identity.strategy_id}:trade:")
    assert generated_row["version"] == 1


def test_trades_delta_sets_reset_required_when_gap_exceeds_bounded_scan(
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
            {"strategy_id": flux_config.identity.strategy_id, "row_id": f"t-{seq}", "seq": seq, "ts_ms": seq * 1_000}
            for seq in range(1, 2_006)
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
        response = client.get("/api/v1/trades/delta", query_string={"since_seq": 0, "limit": 50})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["reset_required"] is True
    assert body["data"]["last_seq"] == 0
    assert body["data"]["rows"] == []


def test_trades_delta_boundary_non_gap_does_not_force_reset(
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
            {"strategy_id": flux_config.identity.strategy_id, "row_id": f"t-{seq}", "seq": seq, "ts_ms": seq * 1_000}
            for seq in range(1, 2_002)
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
        response = client.get("/api/v1/trades/delta", query_string={"since_seq": 1, "limit": 3})
        body = response.get_json()

    assert response.status_code == 200
    assert body["data"]["reset_required"] is False
    assert [row["seq"] for row in body["data"]["rows"]] == [2, 3, 4]
    assert body["data"]["last_seq"] == 4


def test_alerts_get_and_delete_return_stable_shapes_for_empty_and_non_empty_sets(
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
    keys = FluxRedisKeys.from_identity(flux_config.identity)

    with app.test_client() as client:
        empty_response = client.get("/api/v1/alerts", query_string={"limit": 50})
        empty_body = empty_response.get_json()

        redis_client.add_stream_rows(
            keys.alerts(),
            [
                {"strategy_id": flux_config.identity.strategy_id, "row_id": "a-1", "severity": "warn", "ts_ms": 10},
                {"strategy_id": flux_config.identity.strategy_id, "row_id": "a-2", "severity": "error", "ts_ms": 20},
            ],
        )
        non_empty_response = client.get("/api/v1/alerts", query_string={"limit": 1})
        non_empty_body = non_empty_response.get_json()

        delete_response = client.delete("/api/v1/alerts")
        delete_body = delete_response.get_json()
        after_delete_response = client.get("/api/v1/alerts")
        after_delete_body = after_delete_response.get_json()

    assert empty_response.status_code == 200
    assert set(empty_body["data"].keys()) >= {"rows", "total", "limit", "offset", "has_more"}
    assert empty_body["data"]["rows"] == []
    assert empty_body["data"]["total"] == 0
    assert empty_body["data"]["has_more"] is False

    assert non_empty_response.status_code == 200
    assert set(non_empty_body["data"].keys()) >= {"rows", "total", "limit", "offset", "has_more"}
    assert non_empty_body["data"]["total"] == 2
    assert len(non_empty_body["data"]["rows"]) == 1
    assert non_empty_body["data"]["has_more"] is True

    assert delete_response.status_code == 200
    assert set(delete_body["data"].keys()) >= {"success", "strategy_id", "deleted", "remaining", "server_ts_ms"}
    assert delete_body["data"]["success"] is True
    assert delete_body["data"]["strategy_id"] == flux_config.identity.strategy_id
    assert isinstance(delete_body["data"]["server_ts_ms"], int)
    assert delete_body["data"]["deleted"] >= 1
    assert delete_body["data"]["remaining"] == 0

    assert after_delete_response.status_code == 200
    assert after_delete_body["data"]["rows"] == []
    assert after_delete_body["data"]["total"] == 0
    assert delete_body["data"]["deleted"] == 2


def test_trades_and_alerts_total_reflect_full_candidate_set_not_page_window(
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
            {"strategy_id": flux_config.identity.strategy_id, "row_id": f"t-{seq}", "seq": seq, "ts_ms": seq * 1_000}
            for seq in range(1, 11)
        ],
    )
    redis_client.add_stream_rows(
        keys.alerts(),
        [
            {"strategy_id": flux_config.identity.strategy_id, "row_id": f"a-{seq}", "severity": "warn", "ts_ms": seq}
            for seq in range(1, 11)
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
        trades_response = client.get("/api/v1/trades", query_string={"offset": 3, "limit": 2})
        trades_body = trades_response.get_json()
        alerts_response = client.get("/api/v1/alerts", query_string={"offset": 4, "limit": 3})
        alerts_body = alerts_response.get_json()

    assert trades_response.status_code == 200
    assert trades_body["data"]["total"] == 10
    assert trades_body["data"]["has_more"] is True
    assert trades_body["data"]["next_offset"] == 5
    assert len(trades_body["data"]["rows"]) == 2

    assert alerts_response.status_code == 200
    assert alerts_body["data"]["total"] == 10
    assert alerts_body["data"]["has_more"] is True
    assert alerts_body["data"]["next_offset"] == 7
    assert len(alerts_body["data"]["rows"]) == 3


def test_internal_errors_do_not_leak_raw_exception_strings(
    flux_config,
    redis_client,
    contract_catalog,
    strategy_metadata,
    params_schema,
    params_defaults,
) -> None:
    redis_client.pipeline_execute_error = RuntimeError("sensitive redis failure details")
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

    assert response.status_code == 500
    assert body["error"]["code"] == "internal_error"
    assert body["error"]["message"] == "Internal server error."
    assert "sensitive redis failure details" not in json.dumps(body)
