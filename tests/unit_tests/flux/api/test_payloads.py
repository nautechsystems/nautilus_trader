from __future__ import annotations

import importlib
import nautilus_trader.flux.api.payloads as payloads
import pytest

from nautilus_trader.flux.api.payloads import ContractCatalogEntry
from nautilus_trader.flux.api.payloads import StrategyMetadata
from nautilus_trader.flux.api.payloads import build_alerts_rows
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


def test_payloads_module_declares_public_exports() -> None:
    flux_api = importlib.import_module("flux.api")
    flux_api_payloads = importlib.import_module("flux.api.payloads")
    nautilus_flux_api = importlib.import_module("nautilus_trader.flux.api")
    nautilus_flux_api_payloads = importlib.import_module("nautilus_trader.flux.api.payloads")

    assert flux_api.payloads is flux_api_payloads
    assert nautilus_flux_api.payloads is nautilus_flux_api_payloads
    assert payloads.__all__ == [
        "ContractCatalogEntry",
        "StrategyMetadata",
        "as_list",
        "build_alerts_rows",
        "build_balances_rows",
        "build_envelope",
        "build_error",
        "build_legs_payload",
        "build_params_payload",
        "build_signals_payload",
        "build_trades_rows",
        "canonical_naming_fields",
        "coerce_ts_ms",
        "collapse_balance_display_rows",
        "contract_id_for_leg",
        "decode_text",
        "enrich_balances_rows",
        "enrich_row_with_canonical_naming",
        "extract_stream_rows",
        "filter_balance_rows_for_contract_scope",
        "load_json",
        "merge_portfolio_balances_rows",
        "normalize_symbol_parts",
        "now_ms",
        "safe_bool",
        "safe_float",
        "safe_int",
        "select_latest_strategy_row",
        "strategy_id_from_row",
    ]


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


def test_build_balances_rows_prefers_base_qty_and_preserves_venue_qty_fields() -> None:
    raw_snapshot = [
        {
            "strategy_id": "strategy_01",
            "exchange": "okx",
            "kind": "position",
            "instrument_id": "PLUME-USDT-SWAP.OKX",
            "signed_qty": "343",
            "quantity": "343",
            "signed_qty_venue": "343",
            "quantity_venue": "343",
            "signed_qty_base": "3430",
            "quantity_base": "3430",
            "qty_conversion_status": "exact_multiplier",
            "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
        },
        {
            "strategy_id": "strategy_01",
            "exchange": "okx",
            "kind": "position",
            "instrument_id": "PLUME-USDT-SWAP.OKX",
            "signed_qty": "-100",
            "quantity": "100",
            "signed_qty_venue": "-100",
            "quantity_venue": "100",
            "signed_qty_base": "-1000",
            "quantity_base": "1000",
            "qty_conversion_status": "exact_multiplier",
            "qty_conversion_source": "instrument.info:base_exposure_mode=exact_multiplier",
        },
    ]

    rows = build_balances_rows(raw_snapshot=raw_snapshot, strategy_id="strategy_01")
    position_rows = [row for row in rows if str(row.get("kind")).lower() == "position"]

    assert len(position_rows) == 1
    assert position_rows[0]["signed_qty"] == "2430"
    assert position_rows[0]["quantity"] == "2430"
    assert position_rows[0]["signed_qty_base"] == "2430"
    assert position_rows[0]["quantity_base"] == "2430"
    assert position_rows[0]["signed_qty_venue"] == "243"
    assert position_rows[0]["quantity_venue"] == "243"
    assert position_rows[0]["qty_conversion_status"] == "exact_multiplier"
    assert (
        position_rows[0]["qty_conversion_source"]
        == "instrument.info:base_exposure_mode=exact_multiplier"
    )


def test_build_balances_rows_preserves_upstream_position_valuation_without_mark() -> None:
    rows = build_balances_rows(
        raw_snapshot=[
            {
                "strategy_id": "strategy_01",
                "exchange": "bybit",
                "kind": "position",
                "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                "signed_qty": "10",
                "quantity": "10",
                "mv_raw": 20.0,
                "notional_quote": 20.0,
            },
        ],
        strategy_id="strategy_01",
    )

    assert len(rows) == 1
    row = rows[0]
    assert row["signed_qty"] == "10"
    assert row["mv_raw"] == 20.0
    assert row["notional_quote"] == 20.0
    assert "mark_raw" not in row


def test_build_balances_rows_flattens_nested_account_events_balances() -> None:
    raw_snapshot = [
        {
            "strategy_id": "strategy_01",
            "accounts": [
                {
                    "account_id": "binance-main",
                    "events": [
                        {
                            "account_id": "binance-main",
                            "ts_ms": 1_700_000_000_123,
                            "balances": [
                                {
                                    "currency": "PLUME",
                                    "free": "-30139.05291039",
                                    "locked": "0",
                                    "total": "-30139.05291039",
                                },
                            ],
                        },
                    ],
                },
            ],
        },
    ]

    rows = build_balances_rows(raw_snapshot=raw_snapshot, strategy_id="strategy_01")
    assert len(rows) == 1
    row = rows[0]
    assert row["strategy_id"] == "strategy_01"
    assert row["exchange"] == "binance"
    assert row["asset"] == "PLUME"
    assert row["coin"] == "PLUME"
    assert row["base"] == "PLUME"
    assert row["free"] == "-30139.05291039"
    assert row["locked"] == "0"
    assert row["total"] == "-30139.05291039"
    assert row["ts_ms"] == 1_700_000_000_123
    assert row["row_id"] == "strategy_01:acc:0:evt:0:0"


def test_build_balances_rows_preserves_account_id_and_formats_stable_cash_without_suffix() -> None:
    raw_snapshot = [
        {
            "strategy_id": "plumeusdt_bitget_perp_makerv3",
            "accounts": [
                {
                    "account_id": "BITGET-001",
                    "events": [
                        {
                            "account_id": "BITGET-001",
                            "ts_ms": 1_700_000_000_456,
                            "balances": [
                                {
                                    "currency": "USDT",
                                    "free": "0",
                                    "locked": "0",
                                    "total": "0",
                                },
                            ],
                        },
                    ],
                },
            ],
        },
    ]

    rows = build_balances_rows(
        raw_snapshot=raw_snapshot,
        strategy_id="plumeusdt_bitget_perp_makerv3",
    )

    assert len(rows) == 1
    row = rows[0]
    assert row["account_id"] == "BITGET-001"
    assert row["account"] == "BITGET-001"
    assert row["product_type"] == "perp"
    assert row["market_type"] == "perp"
    assert row["display_name_short"] == "USDT"
    assert row["display_name_long"] == "Bitget USDT"


def test_enrich_balances_rows_formats_instrumentless_stable_spot_rows_without_suffix() -> None:
    rows = [
        {
            "strategy_id": "plumeusdt_binance_spot_makerv3",
            "exchange": "binance_spot",
            "account_id": "BINANCE_SPOT-MARGIN-master",
            "asset": "USDT",
            "free": "1285.28070703",
            "locked": "0",
            "total": "1285.28070703",
            "product_type": "spot",
            "market_type": "spot",
            "row_id": "row-1",
        },
    ]

    enriched = enrich_balances_rows(rows, contracts=[], market_rows={})

    assert len(enriched) == 1
    row = enriched[0]
    assert row["display_name_short"] == "USDT"
    assert row["display_name_long"] == "Binance USDT"


def test_build_signals_payload_does_not_fabricate_quote_status_from_scalar_count(
    monkeypatch,
) -> None:
    monkeypatch.setattr("flux.api.payloads.now_ms", lambda: 1_700_000_000_000)
    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=StrategyMetadata(
            strategy_class="MakerV3Strategy",
            strategy_groups="maker",
            base_asset="PLUME",
            quote_asset="USDT",
        ),
        state={
            "bot_on": True,
            "managed_orders": 10,
            "ts_ms": 1_700_000_000_000,
        },
        fv_row={},
        params={"n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs={},
    )

    assert payload["maker_quote_status"] is None


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


def test_merge_portfolio_balances_rows_recomputes_netted_position_mv_from_mark() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "10",
                    "quantity": "10",
                    "mark_raw": 2.0,
                    "mv_raw": 20.0,
                },
            ],
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "-5",
                    "quantity": "5",
                    "mark_raw": 2.0,
                    "mv_raw": -10.0,
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert position["signed_qty"] == "5"
    assert position["mark_raw"] == pytest.approx(2.0)
    assert position["mv_raw"] == pytest.approx(10.0)


def test_merge_portfolio_balances_rows_preserves_upstream_position_valuation_without_mark() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "10",
                    "quantity": "10",
                    "mv_raw": 20.0,
                    "notional_quote": 20.0,
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert position["signed_qty"] == "10"
    assert position["mv_raw"] == 20.0
    assert position["notional_quote"] == 20.0
    assert "mark_raw" not in position


def test_merge_portfolio_balances_rows_nets_unmarked_position_valuation_when_all_rows_provide_it() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "10",
                    "quantity": "10",
                    "mv_raw": 20.0,
                    "notional_quote": 20.0,
                },
            ],
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "-5",
                    "quantity": "5",
                    "mv_raw": -10.0,
                    "notional_quote": -10.0,
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert position["signed_qty"] == "5"
    assert position["mv_raw"] == 10.0
    assert position["notional_quote"] == 10.0
    assert "mark_raw" not in position


def test_merge_portfolio_balances_rows_labels_realized_only_pnl_without_unrealized_marker() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "kind": "position",
                    "exchange": "bybit",
                    "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
                    "signed_qty": "2",
                    "quantity": "2",
                    "realized_pnl": "5.5",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    position = rows_by_id["tokenmm:pos:bybit:PLUMEUSDT-LINEAR.BYBIT"]
    assert "uPnL=" not in position["locked"]
    assert "rPnL=5.5" in position["locked"]


def test_merge_portfolio_balances_rows_retains_latest_known_mark_when_newer_cash_row_lacks_one() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "exchange": "bybit",
                    "account": "main",
                    "asset": "PLUME",
                    "total": "3434.3519",
                    "ts_ms": 1_700_000_000_000,
                    "mark_raw": 0.0106,
                    "mv_raw": 36.40413014,
                    "row_id": "strategy_01:cash:0",
                },
            ],
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "exchange": "bybit",
                    "account": "main",
                    "asset": "PLUME",
                    "total": "3434.3519",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "strategy_02:cash:0",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    cash = rows_by_id["tokenmm:cash:bybit:main:PLUME"]

    assert cash["strategy_id"] == "tokenmm"
    assert cash["mark_raw"] == pytest.approx(0.0106)
    assert cash["mv_raw"] == pytest.approx(36.40413014)


def test_merge_portfolio_balances_rows_backfills_latest_cash_mark_even_when_sparse_row_arrives_first() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "strategy_02": [
                {
                    "strategy_id": "strategy_02",
                    "exchange": "bybit",
                    "asset": "PLUME",
                    "free": "6431.15191",
                    "locked": "0",
                    "total": "6431.15191",
                    "ts_ms": 1_700_000_000_200,
                    "row_id": "strategy_02:cash:0",
                },
            ],
            "strategy_01": [
                {
                    "strategy_id": "strategy_01",
                    "exchange": "bybit",
                    "asset": "PLUME",
                    "free": "5434.35191",
                    "locked": "0",
                    "total": "5434.35191",
                    "ts_ms": 1_700_000_000_100,
                    "mark_raw": 0.0106,
                    "mv_raw": 57.604130246,
                    "row_id": "strategy_01:cash:0",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    rows_by_id = {row["row_id"]: row for row in merged}
    cash = rows_by_id["tokenmm:cash:bybit::PLUME"]

    assert cash["strategy_id"] == "tokenmm"
    assert cash["total"] == "6431.15191"
    assert cash["mark_raw"] == pytest.approx(0.0106)
    assert cash["mv_raw"] == pytest.approx(68.170210246)


def test_merge_portfolio_balances_rows_merges_same_account_stable_cash_across_product_scopes() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "plumeusdt_bitget_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_spot_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "500",
                    "locked": "0",
                    "total": "500",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "plumeusdt_bitget_spot_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
            "plumeusdt_bitget_perp_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_perp_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "0",
                    "locked": "0",
                    "total": "0",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_bitget_perp_makerv3:cash:0",
                    "product_type": "perp",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    cash_rows = [
        row
        for row in merged
        if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
    ]

    assert len(cash_rows) == 1
    row = cash_rows[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["total"] == "500"
    assert row["display_name_short"] == "USDT"
    assert row["display_name_long"] == "Bitget USDT"


def test_merge_portfolio_balances_rows_keeps_non_zero_stable_cash_when_newer_duplicate_scope_reports_zero() -> None:
    merged = merge_portfolio_balances_rows(
        rows_by_strategy={
            "plumeusdt_bitget_spot_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_spot_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "500",
                    "locked": "0",
                    "total": "500",
                    "ts_ms": 1_700_000_000_000,
                    "row_id": "plumeusdt_bitget_spot_makerv3:cash:0",
                    "product_type": "spot",
                },
            ],
            "plumeusdt_bitget_perp_makerv3": [
                {
                    "strategy_id": "plumeusdt_bitget_perp_makerv3",
                    "exchange": "bitget",
                    "account_id": "BITGET-001",
                    "asset": "USDT",
                    "free": "0",
                    "locked": "0",
                    "total": "0",
                    "ts_ms": 1_700_000_000_100,
                    "row_id": "plumeusdt_bitget_perp_makerv3:cash:0",
                    "product_type": "perp",
                },
            ],
        },
        portfolio_id="tokenmm",
    )

    cash_rows = [
        row
        for row in merged
        if row.get("exchange") == "bitget" and row.get("asset") == "USDT"
    ]

    assert len(cash_rows) == 1
    row = cash_rows[0]
    assert row["row_id"] == "tokenmm:cash:bitget:BITGET-001:USDT"
    assert row["total"] == "500"


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


def test_enrich_balances_rows_recomputes_position_mv_from_signed_qty_even_when_prefilled() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "PLUMEUSDT-LINEAR.BYBIT",
            "signed_qty": "-162162.2",
            "quantity": "162162.2",
            "side": "SHORT",
            "mark_raw": 0.010704,
            "mv_raw": 706.45,
            "row_id": "strategy_01:pos:prefilled",
        },
    ]
    contracts = (
        ContractCatalogEntry(
            exchange="bybit",
            symbol="PLUME/USDT",
            instrument_id="PLUMEUSDT-LINEAR.BYBIT",
        ),
    )

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows={},
    )

    plume_perp = enriched[0]
    assert plume_perp["mark_raw"] == pytest.approx(0.010704)
    assert plume_perp["mv_raw"] == pytest.approx(-1735.7841888)


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


def test_filter_balance_rows_for_contract_scope_keeps_usdc_collateral_for_usd_perps() -> None:
    rows = [
        {
            "row_id": "cash-usdc",
            "exchange": "hyperliquid",
            "asset": "USDC",
            "total": "250.5",
        },
        {
            "row_id": "cash-zent",
            "exchange": "hyperliquid",
            "asset": "ZENT",
            "total": "500",
        },
        {
            "row_id": "pos-aapl",
            "exchange": "hyperliquid",
            "kind": "position",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "asset": "AAPL",
            "signed_qty": "1",
        },
    ]

    filtered = filter_balance_rows_for_contract_scope(
        rows,
        contracts=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
        ),
    )

    assert [row["row_id"] for row in filtered] == [
        "cash-usdc",
        "pos-aapl",
    ]


def test_filter_balance_rows_for_contract_scope_keeps_stable_collateral_for_usd_perps() -> None:
    rows = [
        {
            "row_id": "cash-usdt",
            "exchange": "hyperliquid",
            "asset": "USDT",
            "total": "125.0",
        },
        {
            "row_id": "cash-usde",
            "exchange": "hyperliquid",
            "asset": "USDE",
            "total": "50.0",
        },
        {
            "row_id": "cash-zent",
            "exchange": "hyperliquid",
            "asset": "ZENT",
            "total": "500",
        },
        {
            "row_id": "pos-aapl",
            "exchange": "hyperliquid",
            "kind": "position",
            "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
            "asset": "AAPL",
            "signed_qty": "1",
        },
    ]

    filtered = filter_balance_rows_for_contract_scope(
        rows,
        contracts=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
        ),
    )

    assert [row["row_id"] for row in filtered] == [
        "cash-usdt",
        "cash-usde",
        "pos-aapl",
    ]


def test_filter_balance_rows_for_contract_scope_preserves_shared_account_rows_when_requested() -> None:
    rows = [
        {
            "row_id": "cash-hkd",
            "exchange": "ibkr",
            "asset": "HKD",
            "total": "85671.33",
            "source_scope": "shared_account",
            "account_scope_id": "ibkr.reference.main",
        },
        {
            "row_id": "pos-f",
            "exchange": "ibkr",
            "kind": "position",
            "instrument_id": "F.NYSE",
            "asset": "F",
            "signed_qty": "-6",
            "source_scope": "shared_account",
            "account_scope_id": "ibkr.reference.main",
        },
        {
            "row_id": "pos-aapl",
            "exchange": "ibkr",
            "kind": "position",
            "instrument_id": "AAPL.NASDAQ",
            "asset": "AAPL",
            "signed_qty": "1",
        },
    ]

    filtered = filter_balance_rows_for_contract_scope(
        rows,
        contracts=(
            ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        preserve_shared_account_rows=True,
    )

    assert [row["row_id"] for row in filtered] == [
        "cash-hkd",
        "pos-f",
        "pos-aapl",
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


def test_build_signals_payload_prefers_signed_qty_base_for_risk_delta(contract_catalog) -> None:
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
        state={"bot_on": True, "managed_orders": 1, "state": "running", "ts_ms": 1700000000000},
        fv_row={"fv": 100.5},
        params={"qty": 1.0},
        balances=[
            {
                "strategy_id": "strategy_01",
                "kind": "position",
                "instrument_id": "ABCUSDT-LINEAR.VENUE_A",
                "signed_qty_venue": "1.25",
                "quantity_venue": "1.25",
                "signed_qty_base": "12.5",
                "quantity_base": "12.5",
                "side": "LONG",
            },
        ],
        legs=legs,
    )

    assert payload["risk_delta"] == 12.5


def test_build_signals_payload_preserves_ibkr_ref_identity_without_ref_leg_data() -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="equities",
        base_asset="AAPL",
        quote_asset="USD",
    )
    legs = build_legs_payload(
        contracts=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        market_rows={
            "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID": {
                "exchange": "hyperliquid",
                "symbol": "AAPL/USD",
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "bid": 255.7,
                "ask": 255.9,
                "ts_ms": 1700000000000,
            },
        },
        now_ms_value=1700000001000,
    )

    payload = build_signals_payload(
        strategy_id="aapl_tradexyz_makerv3",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "waiting_for_ref_data",
            "ts_ms": 1700000000000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
            },
            "maker_v3": {
                "quote_snapshot": {
                    "maker_exchange": "hyperliquid",
                    "maker_symbol": "AAPL/USD",
                    "ref_exchange": "ibkr",
                    "ref_symbol": "AAPL.NASDAQ",
                },
            },
        },
        fv_row={"fv": 255.8},
        params={"qty": 100},
        balances=[],
        legs=legs,
    )

    assert payload["maker_role_map"]["maker_leg"] == "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID"
    assert payload["maker_role_map"]["ref_leg"] == "ibkr:AAPL.NASDAQ"
    assert payload["maker_v3"]["quote_snapshot"]["maker_exchange"] == "hyperliquid"
    assert payload["maker_v3"]["quote_snapshot"]["maker_symbol"] == "AAPL/USD"
    assert payload["maker_v3"]["quote_snapshot"]["ref_exchange"] == "ibkr"
    assert payload["maker_v3"]["quote_snapshot"]["ref_symbol"] == "AAPL/USD"
    assert payload["maker_v3"]["quote_snapshot"].get("ref_bid") is None
    assert payload["maker_v3"]["quote_snapshot"].get("ref_ask") is None
    assert payload["maker_v3"]["quote_snapshot"]["maker_top_bid"] == 255.7
    assert payload["maker_v3"]["quote_snapshot"]["maker_top_ask"] == 255.9


def test_strategy_metadata_payload_includes_param_set_and_strategy_version() -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="equities",
        base_asset="AAPL",
        quote_asset="USD",
        param_set="makerv3",
        strategy_family="maker_v3",
        strategy_version="v3",
    )

    assert metadata.as_payload(strategy_id="aapl_tradexyz_makerv3") == {
        "strategy_id": "aapl_tradexyz_makerv3",
        "class": "maker_v3",
        "strategy_groups": "equities",
        "base_asset": "AAPL",
        "quote_asset": "USD",
        "param_set": "makerv3",
        "strategy_family": "maker_v3",
        "strategy_version": "v3",
    }


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

    assert payload["maker_quote_status"] is None


def test_build_signals_payload_backfills_local_qty_from_pricing_debug_when_state_adjustments_sparse(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1700000000000,
            "pricing_adjustments": [
                {
                    "type": "inventory_skew",
                },
            ],
            "pricing_debug": {
                "skew": {
                    "inventory_qty": "215144.93330847",
                    "local_inventory_qty": "-9806",
                    "local_ratio": "-0.19612",
                    "local_skew_bps": "-4.903",
                    "des_qty_global": "0",
                    "max_qty_global": "100000",
                    "max_skew_bps_global": "25",
                    "des_qty_local": "0",
                    "max_qty_local": "50000",
                    "max_skew_bps_local": "25",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    adjustments = payload["pricing_adjustments"]
    assert adjustments
    assert adjustments[0]["type"] == "inventory_skew"
    assert adjustments[0]["local_qty"] == -9806.0
    assert adjustments[0]["inv_ratio_local"] == -0.19612
    assert adjustments[0]["inv_skew_local"] == -4.903


def test_build_signals_payload_prefers_explicit_base_inventory_fields_when_present(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1700000000000,
            "pricing_adjustments": [
                {
                    "type": "inventory_skew",
                },
            ],
            "pricing_debug": {
                "skew": {
                    "inventory_qty_base": "3430",
                    "inventory_qty": "343",
                    "global_inventory_qty_base": "3430",
                    "global_inventory_qty": "343",
                    "local_inventory_qty_base": "-98060",
                    "local_inventory_qty": "-9806",
                    "global_ratio": "0.0686",
                    "global_skew_bps": "1.372",
                    "local_ratio": "-0.9806",
                    "local_skew_bps": "0",
                    "des_qty_global": "0",
                    "max_qty_global": "50000",
                    "max_skew_bps_global": "20",
                    "des_qty_local": "0",
                    "max_qty_local": "100000",
                    "max_skew_bps_local": "0",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    adjustments = payload["pricing_adjustments"]
    assert adjustments
    assert adjustments[0]["type"] == "inventory_skew"
    assert adjustments[0]["global_qty"] == 3430.0
    assert adjustments[0]["curr_qty"] == 3430.0
    assert adjustments[0]["local_qty"] == -98060.0


def test_build_signals_payload_backfills_global_qty_from_global_inventory_qty_when_sparse(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "bot_on": False,
            "managed_orders": 0,
            "state": "bot_off",
            "ts_ms": 1700000000000,
            "pricing_adjustments": [
                {
                    "type": "inventory_skew",
                },
            ],
            "pricing_debug": {
                "skew": {
                    "global_inventory_qty": "134961.863",
                    "local_inventory_qty": "-62145.1373",
                    "global_ratio": "1",
                    "global_skew_bps": "25",
                    "local_ratio": "-0.2485805492",
                    "local_skew_bps": "-7.457416476",
                    "des_qty_global": "0",
                    "max_qty_global": "100000",
                    "max_skew_bps_global": "25",
                    "des_qty_local": "0",
                    "max_qty_local": "250000",
                    "max_skew_bps_local": "30",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    adjustments = payload["pricing_adjustments"]
    assert adjustments
    assert adjustments[0]["type"] == "inventory_skew"
    assert adjustments[0]["global_qty"] == 134961.863
    assert adjustments[0]["curr_qty"] == 134961.863
    assert adjustments[0]["local_qty"] == -62145.1373
    assert adjustments[0]["inv_ratio_global"] == 1.0
    assert adjustments[0]["inv_skew_global"] == 25.0


def test_build_signals_payload_prefers_canonical_base_qty_fields_when_present(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "managed_orders": 1,
            "state": "running",
            "ts_ms": 1700000000000,
            "local_qty_base": "15.5",
            "global_qty_base": "40.25",
            "global_qty_base_complete": False,
            "aggregation_mode": "partial",
            "pricing_adjustments": [
                {
                    "type": "inventory_skew",
                },
            ],
            "pricing_debug": {
                "skew": {
                    "global_inventory_qty": "999",
                    "local_inventory_qty": "999",
                    "des_qty_global": "0",
                    "max_qty_global": "100000",
                    "max_skew_bps_global": "25",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    adjustments = payload["pricing_adjustments"]
    assert adjustments
    assert adjustments[0]["type"] == "inventory_skew"
    assert adjustments[0]["local_qty_base"] == 15.5
    assert adjustments[0]["local_qty"] == 15.5
    assert adjustments[0]["global_qty_base"] == 40.25
    assert adjustments[0]["global_qty"] == 40.25
    assert adjustments[0]["curr_qty"] == 40.25
    assert adjustments[0]["global_qty_base_complete"] is False
    assert adjustments[0]["global_qty_complete"] is False
    assert adjustments[0]["aggregation_mode"] == "partial"


def test_build_signals_payload_backfills_inventory_skew_when_state_adjustments_list_empty(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "bot_on": False,
            "managed_orders": 0,
            "state": "running",
            "ts_ms": 1700000000000,
            "pricing_adjustments": [],
            "pricing_debug": {
                "pricing": {
                    "bid_edge1_cfg_bps": "10",
                    "ask_edge1_cfg_bps": "10",
                    "bid_edge1_eff_bps": "8",
                    "ask_edge1_eff_bps": "12",
                },
                "skew": {
                    "global_inventory_qty": "134961.863",
                    "local_inventory_qty": "-62145.1373",
                    "global_ratio": "1",
                    "global_skew_bps": "25",
                    "local_ratio": "-0.2485805492",
                    "local_skew_bps": "-7.457416476",
                    "total_skew_bps": "17.542583524",
                    "des_qty_global": "0",
                    "max_qty_global": "100000",
                    "max_skew_bps_global": "25",
                    "des_qty_local": "0",
                    "max_qty_local": "250000",
                    "max_skew_bps_local": "30",
                },
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    adjustments = payload["pricing_adjustments"]
    assert adjustments
    assert adjustments[0]["type"] == "inventory_skew"
    assert adjustments[0]["global_qty"] == 134961.863
    assert adjustments[0]["local_qty"] == -62145.1373
    assert adjustments[0]["delta_bid_edge_bps"] == -2.0
    assert adjustments[0]["delta_ask_edge_bps"] == 2.0


def test_build_signals_payload_zeroes_quote_counts_when_state_is_stale_and_no_live_legs(
    contract_catalog,
    monkeypatch,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="ABC",
        quote_asset="USDT",
    )
    stale_ts_ms = 1_700_000_000_000
    monkeypatch.setattr("flux.api.payloads.now_ms", lambda: stale_ts_ms + 45_000)

    legs = build_legs_payload(
        contracts=contract_catalog,
        market_rows={},
        now_ms_value=stale_ts_ms + 45_000,
    )

    payload = build_signals_payload(
        strategy_id="strategy_01",
        metadata=metadata,
        state={
            "bot_on": True,
            "managed_orders": 1022,
            "state": "quotes_replaced",
            "ts_ms": stale_ts_ms,
            "maker_quote_status": {
                "bid_open": 10,
                "ask_open": 10,
                "bid_depth": 10,
                "ask_depth": 10,
                "bid_blocked": 0,
                "ask_blocked": 0,
            },
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0, "bot_on": False},
        balances=[],
        legs=legs,
    )

    assert payload["managed_orders"] == 0
    assert payload["tradeable"] is False
    assert payload["blocked"] is True
    assert payload["maker_quote_status"] == {
        "bid_open": 0,
        "ask_open": 0,
        "bid_depth": 0,
        "ask_depth": 0,
        "bid_blocked": 0,
        "ask_blocked": 0,
    }
    assert payload["debug"]["md_health"]["state_stale"] is True


def test_build_signals_payload_marks_blocked_reconciliation_as_not_tradeable(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "managed_orders": 2,
            "state": "blocked_reconciliation",
            "ts_ms": 1700000000000,
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    assert payload["tradeable"] is False
    assert payload["blocked"] is True
    assert payload["state"]["state"] == "blocked_reconciliation"


def test_build_signals_payload_marks_quote_blockers_as_not_tradeable(
    contract_catalog,
) -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v3",
        strategy_groups="tokenmm",
        base_asset="PLUME",
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
            "managed_orders": 0,
            "state": "running",
            "ts_ms": 1700000000000,
            "quote_blockers": [
                {
                    "reason_code": "pending_cancel_stuck",
                    "pending_cancel_count": 1,
                    "oldest_pending_cancel_age_ms": 60_000,
                },
            ],
        },
        fv_row={"fv": 100.5},
        params={"qty": 1.0, "n_orders1": 5, "n_orders2": 0, "n_orders3": 0},
        balances=[],
        legs=legs,
    )

    assert payload["tradeable"] is False
    assert payload["blocked"] is True
    assert payload["state"]["quote_blockers"][0]["reason_code"] == "pending_cancel_stuck"


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


def test_build_legs_payload_uses_module_clock_when_now_ms_value_missing(monkeypatch) -> None:
    monkeypatch.setattr("flux.api.payloads.now_ms", lambda: 1_700_000_010_000)

    legs = build_legs_payload(
        contracts=(ContractCatalogEntry(exchange="venue_a", symbol="ABC/USDT"),),
        market_rows={
            "venue_a:ABC/USDT": {"bid": 100.0, "ask": 101.0, "ts_ms": 1_700_000_000_000},
        },
    )

    assert legs["venue_a:ABC/USDT"]["age_ms"] == 10_000


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


def test_enrich_balances_rows_prefers_canonical_spot_contract_with_live_market_for_cash_rows() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "account": "main",
            "asset": "PLUME",
            "total": "5",
            "row_id": "strategy_01:acc:0",
        },
    ]
    contracts = (
        ContractCatalogEntry(
            exchange="bybit",
            symbol="PLUME/USDC",
            instrument_id="PLUMEUSDC-SPOT.BYBIT",
        ),
        ContractCatalogEntry(
            exchange="bybit",
            symbol="PLUME/USDT",
            instrument_id="PLUMEUSDT-SPOT.BYBIT",
        ),
    )
    market_rows = {
        "bybit:PLUMEUSDT-SPOT.BYBIT": {"bid": 0.0104, "ask": 0.0106, "ts_ms": 2_000},
    }

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows=market_rows,
    )

    plume_cash = enriched[0]
    assert plume_cash["instrument_id"] == "PLUMEUSDT-SPOT.BYBIT"
    assert plume_cash["quote_asset"] == "USDT"
    assert plume_cash["mark_raw"] == pytest.approx(0.0105)
    assert plume_cash["mv_raw"] == pytest.approx(0.0525)


def test_enrich_balances_rows_prefers_spot_contract_and_market_fallback_for_spot_position_rows() -> None:
    rows = [
        {
            "strategy_id": "strategy_01",
            "exchange": "bybit",
            "kind": "position",
            "instrument_id": "PLUMEUSDT.BYBIT",
            "asset": "PLUME",
            "signed_qty": "-32311.0667",
            "quantity": "32311.0667",
            "side": "SHORT",
            "avg_px_open": "0",
            "row_id": "spot-pos",
        },
    ]
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
    )
    market_rows = {
        "bybit:PLUMEUSDT-SPOT.BYBIT": {"bid": 0.0104, "ask": 0.0106},
        "bybit:PLUMEUSDT-LINEAR.BYBIT": {"bid": 0.0103, "ask": 0.0105},
    }

    enriched = enrich_balances_rows(
        rows,
        contracts=contracts,
        market_rows=market_rows,
    )

    plume_spot = enriched[0]
    assert plume_spot["instrument_id"] == "PLUMEUSDT-SPOT.BYBIT"
    assert plume_spot["product_type"] == "spot"
    assert plume_spot["display_name_short"] == "PLUME Spot"
    assert plume_spot["mark_raw"] == pytest.approx(0.0105)
    assert plume_spot["mv_raw"] == pytest.approx(-339.26620035)


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


def test_build_alerts_rows_uses_entry_id_as_stable_row_identity_when_row_id_is_missing() -> None:
    rows = build_alerts_rows(
        rows=[
            {
                "strategy_id": "strategy_01",
                "entry_id": "1700000000001-0",
                "level": "error",
                "message": "borrow denied",
                "ts_ms": 1_700_000_000_001,
            },
        ],
        strategy_id="strategy_01",
        limit=10,
    )

    assert rows[0]["row_id"] == "1700000000001-0"
    assert rows[0]["id"] == "1700000000001-0"


def test_build_signals_payload_emits_makerv4_quote_snapshot() -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v4",
        strategy_groups="equities",
        base_asset="AAPL",
        quote_asset="USD",
        param_set="makerv4",
        strategy_family="maker_v4",
        strategy_version="v4",
    )
    legs = build_legs_payload(
        contracts=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        market_rows={
            "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID": {
                "exchange": "hyperliquid",
                "symbol": "AAPL/USD",
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "bid": 255.7,
                "ask": 255.9,
                "ts_ms": 1700000000000,
            },
            "ibkr:AAPL.NASDAQ": {
                "exchange": "ibkr",
                "symbol": "AAPL/USD",
                "instrument_id": "AAPL.NASDAQ",
                "bid": 255.6,
                "ask": 255.8,
                "ts_ms": 1700000000001,
            },
        },
        now_ms_value=1700000001000,
    )

    payload = build_signals_payload(
        strategy_id="aapl_tradexyz_makerv4",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "hedge_paused",
            "ts_ms": 1700000000000,
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
                    "fee_snapshot_age_s": 9,
                    "hedge_latency_ms": 45,
                    "hedge_slippage_bps_vs_mid": 1.5,
                    "hedge_leg": {
                        "venue": "IBKR",
                        "instrument_id": "AAPL.NASDAQ",
                        "route": "BLUEOCEAN",
                    },
                },
            },
        },
        fv_row={"fv": 255.8},
        params={"qty": 1.0},
        balances=[],
        legs=legs,
    )

    assert payload["strategy_family"] == "maker_v4"
    quote_snapshot = payload["maker_v4"]["quote_snapshot"]
    assert quote_snapshot["maker_leg"]["venue"] == "HYPERLIQUID"
    assert quote_snapshot["hedge_leg"]["venue"] == "IBKR"
    assert quote_snapshot["hedge_leg"]["route"] == "BLUEOCEAN"
    assert quote_snapshot["ref_leg"]["venue"] == "IBKR"
    assert quote_snapshot["effective_spread_bps"] == 6.5
    assert quote_snapshot["effective_account_source"] == "userRole.master"
    assert quote_snapshot["assumed_hedge_fee_bps"] == 1.0
    assert quote_snapshot["hedge_disabled_reason"] == "stale_quote"


def test_build_signals_payload_synthesizes_distinct_makerv4_hedge_leg_from_role_map() -> None:
    metadata = StrategyMetadata(
        strategy_class="maker_v4",
        strategy_groups="equities",
        base_asset="AAPL",
        quote_asset="USD",
        param_set="makerv4",
        strategy_family="maker_v4",
        strategy_version="v4",
    )
    legs = build_legs_payload(
        contracts=(
            ContractCatalogEntry(
                exchange="hyperliquid",
                symbol="AAPL/USD",
                instrument_id="xyz:AAPL-USD-PERP.HYPERLIQUID",
            ),
            ContractCatalogEntry(
                exchange="ibkr",
                symbol="AAPL/USD",
                instrument_id="AAPL.NASDAQ",
            ),
        ),
        market_rows={
            "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID": {
                "exchange": "hyperliquid",
                "symbol": "AAPL/USD",
                "instrument_id": "xyz:AAPL-USD-PERP.HYPERLIQUID",
                "bid": 255.7,
                "ask": 255.9,
                "ts_ms": 1700000000000,
            },
            "ibkr:AAPL.NASDAQ": {
                "exchange": "ibkr",
                "symbol": "AAPL/USD",
                "instrument_id": "AAPL.NASDAQ",
                "bid": 255.6,
                "ask": 255.8,
                "ts_ms": 1700000000001,
            },
        },
        now_ms_value=1700000001000,
    )

    payload = build_signals_payload(
        strategy_id="aapl_tradexyz_makerv4",
        metadata=metadata,
        state={
            "bot_on": False,
            "managed_orders": 0,
            "state": "hedge_paused",
            "ts_ms": 1700000000000,
            "maker_role_map": {
                "maker_leg": "hyperliquid:XYZ:AAPL-USD-PERP.HYPERLIQUID",
                "ref_leg": "AAPL.NASDAQ",
                "hedge_leg": "AAPL.BLUEOCEAN",
            },
            "maker_v4": {
                "quote_snapshot": {
                    "effective_spread_bps": -0.72,
                    "quoted_spread_bps": 1.05,
                    "expected_maker_fee_bps": 0.25,
                    "assumed_hedge_fee_bps": 1.0,
                    "hedge_route": "BLUEOCEAN",
                    "fee_snapshot_age_s": 0.025,
                    "hedge_latency_ms": 40,
                    "hedge_slippage_bps_vs_mid": 0.53,
                },
            },
        },
        fv_row={"fv": 255.8},
        params={"qty": 1.0},
        balances=[],
        legs=legs,
    )

    quote_snapshot = payload["maker_v4"]["quote_snapshot"]
    assert quote_snapshot["hedge_leg"]["instrument_id"] == "AAPL.BLUEOCEAN"
    assert quote_snapshot["hedge_leg"]["venue"] == "IBKR"
    assert quote_snapshot["hedge_leg"]["symbol"] == "AAPL/USD"
    assert quote_snapshot["ref_leg"]["instrument_id"] == "AAPL.NASDAQ"
    assert quote_snapshot["hedge_route"] == "BLUEOCEAN"
