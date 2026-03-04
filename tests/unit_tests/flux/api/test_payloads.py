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

from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.api.payloads import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import build_balances_rows
from nautilus_trader.flux.api.payloads import build_envelope
from nautilus_trader.flux.api.payloads import build_legs_payload
from nautilus_trader.flux.api.payloads import build_signals_payload


def test_build_envelope_includes_standard_fields() -> None:
    envelope = build_envelope(
        ok=True,
        request_id="req-1",
        timestamp_ms=1700000000000,
        api_version="v1",
        data={"value": 1},
        error=None,
    )

    assert envelope == {
        "ok": True,
        "api_version": "v1",
        "request_id": "req-1",
        "timestamp_ms": 1700000000000,
        "data": {"value": 1},
        "error": None,
    }


def test_build_balances_rows_flattens_events_and_aggregates_positions() -> None:
    raw_snapshot = [
        {
            "strategy_id": "strategy_01",
            "events": [
                {
                    "account_id": "venue_a-main",
                    "balances": [
                        {"currency": "abc", "free": "10", "locked": "0", "total": "10"},
                    ],
                },
            ],
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "venue_a",
            "kind": "position",
            "asset": "ABC-PERP",
            "signed_qty": "2.5",
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "venue_a",
            "kind": "position",
            "asset": "ABC-PERP",
            "signed_qty": "-1.0",
        },
    ]

    rows = build_balances_rows(raw_snapshot=raw_snapshot, strategy_id="strategy_01")
    position_rows = [row for row in rows if str(row.get("kind")).lower() == "position"]
    spot_rows = [row for row in rows if str(row.get("kind")).lower() != "position"]

    assert len(position_rows) == 1
    assert position_rows[0]["signed_qty"] == "1.5"
    assert position_rows[0]["side"] == "LONG"
    assert len(spot_rows) == 1
    assert spot_rows[0]["exchange"] == "venue_a"
    assert spot_rows[0]["asset"] == "ABC"


def test_build_signals_payload_uses_injected_metadata_and_legs(contract_catalog) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={
            "venue_a:ABC/USDT": {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000000},
            "venue_b:ABC/USDT": {"bid": 99.0, "ask": 100.0, "ts_ms": 1700000000100},
        },
        now_ms_value=1700000001000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={"bot_on": True, "managed_orders": 3, "state": "running", "ts_ms": 1700000000000},
        fv_row={"fv": 100.5},
        params={"qty": 1.0},
        balances=[],
        legs=legs,
    )

    assert payload["id"] == "strategy_01"
    assert payload["meta"] == {
        "strategy_id": "strategy_01",
        "class": "maker_v3",
        "strategy_groups": "tokenmm",
        "base_asset": "ABC",
        "quote_asset": "USDT",
    }
    assert payload["tradeable"] is True
    assert payload["managed_orders"] == 3
    assert payload["legs"]["venue_a:ABC/USDT"]["mid"] == 100.5


def test_build_legs_payload_uses_contract_id_keys_for_same_exchange_contracts() -> None:
    contracts = (
        ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),
        ContractCatalogEntry(exchange="venue_a", symbol="XYZ/USDT"),
    )

    legs = build_legs_payload(
        contracts=contracts,
        market_rows={
            "venue_a:ABC/USDT": {"bid": 100.0, "ask": 101.0, "ts_ms": 1700000000000},
            "venue_a:XYZ/USDT": {"bid": 200.0, "ask": 201.0, "ts_ms": 1700000000100},
        },
        now_ms_value=1700000001000,
    )

    assert set(legs.keys()) == {"venue_a:ABC/USDT", "venue_a:XYZ/USDT"}
    assert legs["venue_a:ABC/USDT"]["exchange"] == "venue_a"
    assert legs["venue_a:ABC/USDT"]["symbol"] == "ABC/USDT"
    assert legs["venue_a:ABC/USDT"]["mid"] == 100.5
    assert legs["venue_a:XYZ/USDT"]["exchange"] == "venue_a"
    assert legs["venue_a:XYZ/USDT"]["symbol"] == "XYZ/USDT"
    assert legs["venue_a:XYZ/USDT"]["mid"] == 200.5
