"""
Provide MakerV3 reconciliation-specific helpers layered above generic inventory logic.
"""

from __future__ import annotations

from collections.abc import Callable
from collections.abc import Iterable
from collections.abc import Mapping
from decimal import Decimal
from typing import Any

from nautilus_trader.live.reconciliation import filter_external_reconciliation_artifacts
from flux.strategies.makerv3.pricing import to_decimal_or_none


def _position_signed_qty_value(position: Any) -> Decimal | None:
    signed_qty = to_decimal_or_none(getattr(position, "signed_qty", None))
    if signed_qty is None:
        qty = to_decimal_or_none(getattr(position, "quantity", None))
        side = str(getattr(position, "side", "") or "").strip().upper()
        if qty is not None:
            signed_qty = -qty if side == "SHORT" else qty
    return signed_qty


def maker_snapshot_signed_qty(
    snapshot: Mapping[str, Any] | None,
    *,
    instrument_id: Any,
) -> Decimal | None:
    """
    Return the signed maker venue quantity from a snapshot for the requested instrument.
    """
    if not isinstance(snapshot, Mapping):
        return None
    if snapshot.get("instrument_id") != instrument_id:
        return None
    return to_decimal_or_none(snapshot.get("signed_qty"))


def effective_maker_positions(
    positions: Iterable[Any],
    *,
    maker_instrument_id: Any,
    expected_venue_qty: Decimal | None,
    order_lookup: Callable[[Any], list[Any]] | None = None,
) -> list[Any]:
    """
    Drop stale EXTERNAL reconciliation artifacts for the maker instrument only when the
    non-EXTERNAL cached qty already matches an authoritative expected venue quantity.
    """
    positions_list = list(positions)
    if expected_venue_qty is None or not positions_list:
        return positions_list

    maker_positions = [
        position
        for position in positions_list
        if getattr(position, "instrument_id", None) == maker_instrument_id
    ]
    if not maker_positions:
        return positions_list

    filtered_positions = filter_external_reconciliation_artifacts(
        maker_positions,
        group_key=lambda _position: maker_instrument_id,
        order_lookup=order_lookup,
    )
    if len(filtered_positions) == len(maker_positions):
        return positions_list

    effective_qty = Decimal(0)
    has_effective_qty = False
    for position in filtered_positions:
        signed_qty = _position_signed_qty_value(position)
        if signed_qty is None:
            continue
        effective_qty += signed_qty
        has_effective_qty = True

    if not has_effective_qty or effective_qty != expected_venue_qty:
        return positions_list

    filtered_object_ids = {id(position) for position in filtered_positions}
    maker_object_ids = {id(position) for position in maker_positions}
    removed_object_ids = maker_object_ids - filtered_object_ids
    if not removed_object_ids:
        return positions_list

    return [position for position in positions_list if id(position) not in removed_object_ids]


__all__ = [
    "effective_maker_positions",
    "maker_snapshot_signed_qty",
]
