from __future__ import annotations

from decimal import Decimal
from types import SimpleNamespace

from nautilus_trader.flux.strategies.makerv3.reconciliation import effective_maker_positions
from nautilus_trader.flux.strategies.makerv3.reconciliation import maker_snapshot_signed_qty
from nautilus_trader.model.identifiers import InstrumentId


def test_maker_snapshot_signed_qty_requires_matching_instrument() -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BITGET")

    assert maker_snapshot_signed_qty(
        {"instrument_id": maker_instrument_id, "signed_qty": Decimal("-250030")},
        instrument_id=maker_instrument_id,
    ) == Decimal("-250030")
    assert maker_snapshot_signed_qty(
        {"instrument_id": InstrumentId.from_str("BTCUSDT-PERP.BITGET"), "signed_qty": Decimal("-1")},
        instrument_id=maker_instrument_id,
    ) is None


def test_effective_maker_positions_drops_artifact_only_when_expected_qty_matches_owned_qty() -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BITGET")
    owned_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        strategy_id="maker",
        position_id="P-OWNED",
    )
    external_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
    )

    filtered = effective_maker_positions(
        [owned_position, external_position],
        maker_instrument_id=maker_instrument_id,
        expected_venue_qty=Decimal("-250030"),
        order_lookup=lambda _position_id: [],
    )

    assert filtered == [owned_position]


def test_effective_maker_positions_keeps_partial_external_fragment_until_qty_matches() -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BITGET")
    owned_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-100"),
        strategy_id="maker",
        position_id="P-OWNED",
    )
    external_fragment = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-50"),
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
    )

    filtered = effective_maker_positions(
        [owned_position, external_fragment],
        maker_instrument_id=maker_instrument_id,
        expected_venue_qty=Decimal("-150"),
        order_lookup=lambda _position_id: [],
    )

    assert filtered == [owned_position, external_fragment]


def test_effective_maker_positions_keeps_non_reconciliation_external_lineage() -> None:
    maker_instrument_id = InstrumentId.from_str("PLUMEUSDT-PERP.BITGET")
    owned_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        strategy_id="maker",
        position_id="P-OWNED",
    )
    external_position = SimpleNamespace(
        instrument_id=maker_instrument_id,
        signed_qty=Decimal("-250030"),
        strategy_id="EXTERNAL",
        position_id="P-EXTERNAL",
    )

    filtered = effective_maker_positions(
        [owned_position, external_position],
        maker_instrument_id=maker_instrument_id,
        expected_venue_qty=Decimal("-250030"),
        order_lookup=lambda position_id: (
            [SimpleNamespace(tags=["VENUE"])] if position_id == "P-EXTERNAL" else []
        ),
    )

    assert filtered == [owned_position, external_position]
