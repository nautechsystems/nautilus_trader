from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.live.reconciliation import collapse_duplicate_netting_position_reports
from nautilus_trader.live.reconciliation import filter_external_reconciliation_artifacts


def test_filter_external_reconciliation_artifacts_drops_external_when_non_external_exists() -> None:
    owned_position = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        strategy_id="maker",
        position_id="P-OWNED",
    )
    external_position = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
    )
    other_instrument_position = SimpleNamespace(
        instrument_id="BTCUSDT-PERP.BITGET",
        strategy_id="maker",
        position_id="P-OTHER",
    )

    filtered = filter_external_reconciliation_artifacts(
        [owned_position, external_position, other_instrument_position],
        order_lookup=lambda _position_id: [],
    )

    assert filtered == [owned_position, other_instrument_position]


def test_filter_external_reconciliation_artifacts_keeps_external_with_non_reconciliation_lineage() -> None:
    owned_position = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        strategy_id="maker",
        position_id="P-OWNED",
    )
    external_position = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
    )

    filtered = filter_external_reconciliation_artifacts(
        [owned_position, external_position],
        order_lookup=lambda position_id: (
            [SimpleNamespace(tags=["VENUE"])] if position_id == "P-EXTERNAL" else []
        ),
    )

    assert filtered == [owned_position, external_position]


def test_collapse_duplicate_netting_position_reports_prefers_newest_nonzero_duplicate() -> None:
    reports, collapse_events = collapse_duplicate_netting_position_reports(
        [
            SimpleNamespace(
                instrument_id="PLUMEUSDT-PERP.BITGET",
                signed_decimal_qty=Decimal("-250030"),
                venue_position_id=None,
                position_id=None,
                ts_last=10,
                ts_init=10,
            ),
            SimpleNamespace(
                instrument_id="PLUMEUSDT-PERP.BITGET",
                signed_decimal_qty=Decimal("-250030"),
                venue_position_id=None,
                position_id=None,
                ts_last=20,
                ts_init=20,
            ),
        ],
    )

    assert len(reports) == 1
    assert reports[0].ts_last == 20
    assert collapse_events == [
        {
            "instrument_id": "PLUMEUSDT-PERP.BITGET",
            "report_count": 2,
            "selected_ts_last": 20,
            "selected_signed_qty": Decimal("-250030"),
            "discarded_flat_duplicates": False,
        },
    ]


def test_collapse_duplicate_netting_position_reports_keeps_reports_with_position_ids() -> None:
    first_report = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        signed_decimal_qty=Decimal("-100"),
        venue_position_id="P-001",
        position_id=None,
        ts_last=10,
        ts_init=10,
    )
    second_report = SimpleNamespace(
        instrument_id="PLUMEUSDT-PERP.BITGET",
        signed_decimal_qty=Decimal("-50"),
        venue_position_id="P-002",
        position_id=None,
        ts_last=20,
        ts_init=20,
    )

    reports, collapse_events = collapse_duplicate_netting_position_reports(
        [first_report, second_report],
    )

    assert reports == [first_report, second_report]
    assert collapse_events == []
