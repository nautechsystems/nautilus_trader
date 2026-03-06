from __future__ import annotations

import pytest

from nautilus_trader.flux.api.payloads import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.api.payloads import build_balances_rows
from nautilus_trader.flux.api.payloads import build_envelope
from nautilus_trader.flux.api.payloads import build_legs_payload
from nautilus_trader.flux.api.payloads import build_signals_payload
from nautilus_trader.flux.api.payloads import build_trades_rows
from nautilus_trader.flux.api.payloads import enrich_balances_rows
from nautilus_trader.flux.api.payloads import extract_stream_rows
from nautilus_trader.flux.api.payloads import filter_balance_rows_for_contract_scope
from nautilus_trader.flux.api.payloads import merge_portfolio_balances_rows


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
                    "ts_ms": 1_700_000_000_000,
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
    assert spot_rows[0]["ts_ms"] == 1_700_000_000_000


def test_merge_portfolio_balances_rows_nets_same_instrument_across_strategies() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "quantity": "2",
                    "side": "LONG",
                },
            ],
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "quantity": "1",
                    "side": "SHORT",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert position["strategy_id"] == "tokenmm"
    assert position["signed_qty"] == "1"
    assert position["quantity"] == "1"
    assert position["side"] == "LONG"


def test_enrich_balances_rows_marks_cash_assets_and_positions_from_market_rows() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "account": "main",
            "asset": "PLUME",
            "total": "5434.35191",
            "row_id": "strategy_01:acc:0",
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "account": "main",
            "asset": "USDT",
            "total": "100",
            "row_id": "strategy_01:acc:1",
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
            "signed_qty": "2",
            "quantity": "2",
            "row_id": "strategy_01:pos:0",
        },
    ]
    contracts = (ContractCatalogEntry(exchange="bybit", symbol="PLUME/USDT"),)
    market_rows = {"bybit:PLUME/USDT": {"bid": 0.0104, "ask": 0.0106}}

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows=market_rows,
    )

    by_row_id = {row["row_id"]: row for row in enriched}
    plume_cash = by_row_id["strategy_01:acc:0"]
    usdt_cash = by_row_id["strategy_01:acc:1"]
    plume_perp = by_row_id["strategy_01:pos:0"]

    assert plume_cash["mark_raw"] == pytest.approx(0.0105)
    assert plume_cash["mv_raw"] == pytest.approx(57.060695055)
    assert usdt_cash["mark_raw"] == pytest.approx(1.0)
    assert usdt_cash["mv_raw"] == pytest.approx(100.0)
    assert plume_perp["asset"] == "PLUME"
    assert plume_perp["mark_raw"] == pytest.approx(0.0105)
    assert plume_perp["mv_raw"] == pytest.approx(0.021)


def test_filter_balance_rows_for_contract_scope_excludes_unrelated_assets() -> None:
    rows = [
        {
            "row_id": "cash-plume",
            "exchange": "bybit",
            "asset": "PLUME",
            "total": "10",
        },
        {
            "row_id": "cash-usdt",
            "exchange": "bybit",
            "asset": "USDT",
            "total": "100",
        },
        {
            "row_id": "cash-zent",
            "exchange": "bybit",
            "asset": "ZENT",
            "total": "500",
        },
        {
            "row_id": "pos-plume",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
            "asset": "PLUME",
            "signed_qty": "2",
        },
        {
            "row_id": "pos-btc",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "BTCUSDT-LINEAR.BYBIT",
            "asset": "BTC",
            "signed_qty": "1",
        },
    ]

    filtered = filter_balance_rows_for_contract_scope(
        rows,
        contracts=(ContractCatalogEntry(exchange="bybit", symbol="PLUME/USDT"),),
    )

    assert [row["row_id"] for row in filtered] == [
        "cash-plume",
        "cash-usdt",
        "pos-plume",
    ]


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


def test_build_signals_payload_derives_inventory_skew_and_quote_snapshot_from_state(
    contract_catalog,
) -> None:
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
                    "local_inventory_qty": "12",
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
    assert skew["local_qty"] == 12.0
    assert skew["delta_bid_edge_bps"] == -2.0
    assert skew["delta_ask_edge_bps"] == 2.0


def test_build_signals_payload_derives_quote_status_from_managed_orders_when_missing(
    contract_catalog,
) -> None:
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
        state={"bot_on": True, "managed_orders": 7, "state": "running", "ts_ms": 1700000000000},
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    assert payload["maker_quote_status"] == {
        "bid_open": 4,
        "ask_open": 3,
        "bid_depth": 5,
        "ask_depth": 5,
        "bid_blocked": 1,
        "ask_blocked": 2,
    }


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


def test_build_legs_payload_derives_canonical_naming_for_plume_spot_and_perp() -> None:
    contracts = (
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
        ContractCatalogEntry(
            exchange="binance_spot",
            symbol="PLUME/USDT",
            instrument_id="PLUMEUSDT.BINANCE_SPOT",
        ),
        ContractCatalogEntry(
            exchange="okx",
            symbol="PLUME/USDT",
            instrument_id="PLUME-USDT-SWAP.OKX",
        ),
    )

    legs = build_legs_payload(
        contracts=contracts,
        market_rows={
            "bybit:PLUMEUSDT-LINEAR.BYBIT": {"bid": 0.0104, "ask": 0.0106, "ts_ms": 1700000000000},
            "bybit:PLUMEUSDT-SPOT.BYBIT": {"bid": 0.0105, "ask": 0.0107, "ts_ms": 1700000000001},
            "binance_spot:PLUMEUSDT.BINANCE_SPOT": {
                "bid": 0.0105,
                "ask": 0.0106,
                "ts_ms": 1700000000002,
            },
            "okx:PLUME-USDT-SWAP.OKX": {"bid": 0.0103, "ask": 0.0105, "ts_ms": 1700000000003},
        },
        now_ms_value=1700000001000,
    )

    assert set(legs) == {
        "bybit:PLUMEUSDT-LINEAR.BYBIT",
        "bybit:PLUMEUSDT-SPOT.BYBIT",
        "binance_spot:PLUMEUSDT.BINANCE_SPOT",
        "okx:PLUME-USDT-SWAP.OKX",
    }
    assert legs["bybit:PLUMEUSDT-LINEAR.BYBIT"]["product_type"] == "perp"
    assert legs["bybit:PLUMEUSDT-LINEAR.BYBIT"]["display_name_short"] == "PLUME Perp"
    assert legs["bybit:PLUMEUSDT-SPOT.BYBIT"]["product_type"] == "spot"
    assert legs["bybit:PLUMEUSDT-SPOT.BYBIT"]["display_name_short"] == "PLUME Spot"
    assert legs["binance_spot:PLUMEUSDT.BINANCE_SPOT"]["venue"] == "BINANCE_SPOT"
    assert legs["okx:PLUME-USDT-SWAP.OKX"]["contract_type"] == "swap"
    assert legs["okx:PLUME-USDT-SWAP.OKX"]["display_name_long"] == "Okx PLUME Perp"


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


def test_build_trades_rows_uses_entry_id_as_seq_fallback_for_delta_filters() -> None:
    rows = build_trades_rows(
        rows=[
            {"strategy_id": "strategy_01", "entry_id": "1772691122334-0", "ts_ms": 1772691122334},
            {"strategy_id": "strategy_01", "entry_id": "1772691122335-0", "ts_ms": 1772691122335},
        ],
        strategy_id="strategy_01",
        limit=10,
        since_ms=None,
        since_seq=7_260_942_837_080_064,
    )

    assert len(rows) == 1
    assert rows[0]["seq"] == 7_260_942_837_084_160
    assert rows[0]["row_id"] == "strategy_01:trade:entry:1772691122335-0"


def test_build_trades_rows_derives_canonical_naming_fields_for_plume_instruments() -> None:
    rows = build_trades_rows(
        rows=[
            {
                "strategy_id": "strategy_01",
                "row_id": "trade-bybit-perp",
                "ts_ms": 1700000000000,
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "exchange": "bybit",
            },
            {
                "strategy_id": "strategy_01",
                "row_id": "trade-bybit-spot",
                "ts_ms": 1700000000001,
                "instrument_id": "PLUMEUSDT-SPOT.BYBIT",
                "exchange": "bybit",
            },
            {
                "strategy_id": "strategy_01",
                "row_id": "trade-binance-spot",
                "ts_ms": 1700000000002,
                "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
                "exchange": "binance_spot",
            },
            {
                "strategy_id": "strategy_01",
                "row_id": "trade-okx-perp",
                "ts_ms": 1700000000003,
                "instrument_id": "PLUME-USDT-SWAP.OKX",
                "exchange": "okx",
            },
        ],
        strategy_id="strategy_01",
        limit=10,
        since_ms=None,
        since_seq=None,
    )

    by_id = {row["row_id"]: row for row in rows}
    assert by_id["trade-bybit-perp"]["product_type"] == "perp"
    assert by_id["trade-bybit-perp"]["display_name_short"] == "PLUME Perp"
    assert by_id["trade-bybit-perp"]["raw_symbol"] == "PLUMEUSDT"
    assert by_id["trade-bybit-perp"]["contract_type"] == "linear"
    assert by_id["trade-bybit-perp"]["instrument_uid"] == "bybit:linear:PLUMEUSDT-LINEAR.BYBIT"
    assert by_id["trade-bybit-spot"]["product_type"] == "spot"
    assert by_id["trade-bybit-spot"]["display_name_short"] == "PLUME Spot"
    assert by_id["trade-bybit-spot"]["raw_symbol"] == "PLUMEUSDT"
    assert by_id["trade-binance-spot"]["venue_root"] == "binance"
    assert by_id["trade-binance-spot"]["contract_type"] == "spot"
    assert by_id["trade-okx-perp"]["quote_asset"] == "USDT"
    assert by_id["trade-okx-perp"]["display_name_long"] == "Okx PLUME Perp"


def test_enrich_balances_rows_adds_canonical_naming_fields() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "account": "main",
            "asset": "PLUME",
            "total": "5434.35191",
            "row_id": "strategy_01:acc:0",
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
            "signed_qty": "2",
            "quantity": "2",
            "row_id": "strategy_01:pos:0",
        },
    ]
    contracts = (
        ContractCatalogEntry(
            exchange="bybit",
            symbol="PLUME/USDT",
            instrument_id="PLUMEUSDT-LINEAR.BYBIT",
        ),
    )
    market_rows = {"bybit:PLUMEUSDT-LINEAR.BYBIT": {"bid": 0.0104, "ask": 0.0106}}

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows=market_rows,
    )

    by_row_id = {row["row_id"]: row for row in enriched}
    plume_cash = by_row_id["strategy_01:acc:0"]
    plume_perp = by_row_id["strategy_01:pos:0"]

    assert plume_cash["product_type"] == "spot"
    assert plume_cash["contract_type"] == "cash"
    assert plume_cash["display_name_short"] == "PLUME Spot"
    assert plume_perp["product_type"] == "perp"
    assert plume_perp["contract_type"] == "linear"
    assert plume_perp["display_name_short"] == "PLUME Perp"
    assert plume_perp["display_name_long"] == "Bybit PLUME Perp"
    assert plume_perp["instrument_uid"] == "bybit:linear:PLUMEUSDT-LINEAR.BYBIT"


def test_build_trades_rows_preserves_alias_exchange_when_derived_from_instrument_id() -> None:
    rows = build_trades_rows(
        rows=[
            {
                "strategy_id": "strategy_01",
                "row_id": "trade-binance-spot",
                "ts_ms": 1700000000000,
                "instrument_id": "PLUMEUSDT.BINANCE_SPOT",
            },
        ],
        strategy_id="strategy_01",
        limit=10,
        since_ms=None,
        since_seq=None,
    )

    assert rows[0]["exchange"] == "binance_spot"
    assert rows[0]["venue"] == "BINANCE_SPOT"


def test_enrich_balances_rows_uses_spot_contract_metadata_for_cash_rows() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "account": "main",
            "asset": "PLUME",
            "total": "5434.35191",
            "row_id": "strategy_01:acc:0",
        },
    ]
    contracts = (
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
    )
    market_rows = {"bybit:PLUMEUSDT-SPOT.BYBIT": {"bid": 0.0104, "ask": 0.0106}}

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows=market_rows,
    )

    plume_cash = enriched[0]
    assert plume_cash["instrument_id"] == "PLUMEUSDT-SPOT.BYBIT"
    assert plume_cash["raw_symbol"] == "PLUMEUSDT"
    assert plume_cash["base_asset"] == "PLUME"
    assert plume_cash["quote_asset"] == "USDT"
    assert plume_cash["pair"] == "PLUME/USDT"
    assert plume_cash["display_name_short"] == "PLUME Spot"


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
            "entry_id": "1700000000000-0",
        },
    ]


def test_extract_stream_rows_preserves_entry_metadata_for_payload_rows() -> None:
    rows = extract_stream_rows(
        [
            (
                b"1700000000001-0",
                {
                    b"payload": b'{"strategy_id":"strategy_01","event":"order_filled"}',
                },
            ),
        ],
    )

    assert rows == [
        {
            "strategy_id": "strategy_01",
            "event": "order_filled",
            "entry_id": "1700000000001-0",
            "_stream_seq": 6_963_200_000_004_096,
        },
    ]
