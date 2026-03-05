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
from nautilus_trader.flux.api.payloads import build_trades_rows
from nautilus_trader.flux.api.payloads import extract_stream_rows


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
        params={"qty": 1.0, "cex_bid_edge": 5.0, "cex_ask_edge": 6.0, "pool_edge": 2.0},
        balances=[
            {
                "strategy_id": "strategy_01",
                "kind": "position",
                "instrument_id": "ABCUSDT-PERP",
                "signed_qty": "12.5",
            },
        ],
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
    assert payload["params"]["qty"] == 1.0
    assert payload["balances_ok"] is True
    assert payload["risk_delta"] == 12.5
    assert payload["decision_edge_bps"] == 0.0
    assert payload["spread_net_bps"] == 0.0
    assert payload["required_edge_bps"] == 7.0
    assert payload["edge2_bps"] == -7.0
    assert payload["spread_net_best_case"] == "case2"
    assert payload["spread_net_case1_bps"] == -198.01980198019803
    assert payload["spread_net_case2_bps"] == 0.0
    assert payload["maker_role_map"] == {
        "maker_leg": "venue_a:ABC/USDT",
        "ref_leg": "venue_b:ABC/USDT",
    }
    assert payload["maker_v3"]["quote_snapshot"]["maker_top_bid"] == 100.0
    assert payload["maker_v3"]["quote_snapshot"]["maker_top_ask"] == 101.0
    assert payload["maker_v3"]["quote_snapshot"]["ref_bid"] == 99.0
    assert payload["maker_v3"]["quote_snapshot"]["ref_ask"] == 100.0
    assert payload["legs"]["venue_a:ABC/USDT"]["mid"] == 100.5


def test_build_signals_payload_derives_inventory_skew_and_quote_snapshot_from_state(contract_catalog) -> None:
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
        state={
            "bot_on": True,
            "managed_orders": 3,
            "state": "running",
            "ts_ms": 1700000000000,
            "pricing_debug": {
                "pricing": {
                    "bid_edge1_cfg_bps": "10",
                    "ask_edge1_cfg_bps": "10",
                    "bid_edge1_eff_bps": "8",
                    "ask_edge1_eff_bps": "12",
                    "maker_top_bid": "100",
                    "maker_top_ask": "101",
                    "ref_bid": "99",
                    "ref_ask": "100",
                },
                "skew": {
                    "inventory_qty": "25",
                    "global_ratio": "0.5",
                    "global_skew_bps": "2.5",
                    "local_ratio": "-0.1",
                    "local_skew_bps": "-0.5",
                    "total_skew_bps": "2.0",
                    "des_qty_global": "0",
                    "max_qty_global": "40000",
                    "max_skew_bps_global": "5",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "place_edge1": 2.0},
        balances=[],
        legs=legs,
    )

    assert payload["maker_v3"]["quote_snapshot"]["place_bid"] == 100.0
    assert payload["maker_v3"]["quote_snapshot"]["place_ask"] == 101.0
    assert payload["maker_v3"]["quote_snapshot"]["eff_bid_edge_bps"] == 8.0
    assert payload["maker_v3"]["quote_snapshot"]["eff_ask_edge_bps"] == 12.0
    assert payload["maker_v3"]["quote_snapshot"]["place_edge_bps"] == 2.0

    adjustments = payload["pricing_adjustments"]
    assert len(adjustments) == 1
    skew = adjustments[0]
    assert skew["type"] == "inventory_skew"
    assert skew["skew_bps_signed"] == 2.0
    assert skew["inv_skew"] == 2.0
    assert skew["inv_ratio_global"] == 0.5
    assert skew["inv_skew_local"] == -0.5
    assert skew["curr_qty"] == 25.0
    assert skew["delta_bid_edge_bps"] == -2.0
    assert skew["delta_ask_edge_bps"] == 2.0


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


def test_build_trades_rows_enforces_row_contract_defaults() -> None:
    rows = build_trades_rows(
        rows=[
            {"strategy_id": "strategy_01", "seq": "101", "ts_ms": 101_000},
            {"strategy_id": "strategy_01", "seq": "102"},
            {"strategy_id": "strategy_01", "row_id": "existing", "version": "3", "ts_ms": 103_000},
        ],
        strategy_id="strategy_01",
        limit=10,
        since_ms=None,
        since_seq=None,
    )

    assert len(rows) == 3
    assert rows[0]["row_id"] == "existing"
    assert rows[0]["version"] == 3
    assert rows[0]["ts_ms"] == 103_000_000

    assert rows[1]["version"] == 1
    assert rows[1]["row_id"] == "strategy_01:trade:101:101000000:1"
    assert rows[1]["ts_ms"] == 101_000_000

    assert rows[2]["version"] == 1
    assert rows[2]["row_id"] == "strategy_01:trade:102:0:1"
    assert rows[2]["ts_ms"] == 0


def test_extract_stream_rows_accepts_flat_field_entries_without_payload_wrapper() -> None:
    rows = extract_stream_rows(
        [
            (
                b"1700000000000-0",
                {
                    b"strategy_id": b"strategy_01",
                    b"row_id": b"trade-1",
                    b"seq": b"101",
                    b"ts_ms": b"1700000000000",
                    b"exchange": b"bybit",
                },
            ),
        ],
    )

    assert rows == [
        {
            "strategy_id": "strategy_01",
            "row_id": "trade-1",
            "seq": 101,
            "ts_ms": 1_700_000_000_000,
            "exchange": "bybit",
        },
    ]
