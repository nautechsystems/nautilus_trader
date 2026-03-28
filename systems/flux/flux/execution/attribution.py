from __future__ import annotations

import sys
from dataclasses import dataclass
from decimal import Decimal
from decimal import InvalidOperation


if __name__ == "flux.execution.attribution":
    sys.modules.setdefault("nautilus_trader.flux.execution.attribution", sys.modules[__name__])
elif __name__ == "nautilus_trader.flux.execution.attribution":
    sys.modules.setdefault("flux.execution.attribution", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class AttributionReservation:
    strategy_id: str
    reserved_qty: Decimal | str | int | float
    reservation_seq: int

    def __post_init__(self) -> None:
        object.__setattr__(self, "strategy_id", _required_text(self.strategy_id, "strategy_id"))
        object.__setattr__(self, "reserved_qty", _coerce_decimal(self.reserved_qty, "reserved_qty"))
        object.__setattr__(self, "reservation_seq", int(self.reservation_seq))


@dataclass(frozen=True, slots=True)
class AttributedFill:
    strategy_id: str
    attributed_qty: Decimal
    remaining_reservation_qty: Decimal
    reservation_seq: int


@dataclass(frozen=True, slots=True)
class SharedNettingFillAttribution:
    controller_scope_id: str
    fill_qty: Decimal
    allocations: tuple[AttributedFill, ...]
    unattributed_qty: Decimal


def allocate_shared_netting_fill(
    *,
    controller_scope_id: str,
    fill_qty: Decimal | str | int | float,
    reservations: tuple[AttributionReservation, ...],
) -> SharedNettingFillAttribution:
    scope_id = _required_text(controller_scope_id, "controller_scope_id")
    normalized_fill_qty = _coerce_decimal(fill_qty, "fill_qty")
    normalized_reservations = tuple(
        sorted(tuple(reservations), key=lambda reservation: reservation.reservation_seq),
    )

    seen_strategy_ids: set[str] = set()
    seen_reservation_seq: set[int] = set()
    for reservation in normalized_reservations:
        if reservation.strategy_id in seen_strategy_ids:
            raise ValueError("duplicate strategy_id reservations are not allowed")
        if reservation.reservation_seq in seen_reservation_seq:
            raise ValueError("duplicate reservation_seq reservations are not allowed")
        if (
            normalized_fill_qty != 0
            and reservation.reserved_qty != 0
            and _decimal_sign(normalized_fill_qty) != _decimal_sign(reservation.reserved_qty)
        ):
            raise ValueError("reserved_qty must have the same sign as fill_qty")
        seen_strategy_ids.add(reservation.strategy_id)
        seen_reservation_seq.add(reservation.reservation_seq)

    remaining_fill_abs = abs(normalized_fill_qty)
    fill_sign = Decimal("1") if normalized_fill_qty >= 0 else Decimal("-1")
    allocations: list[AttributedFill] = []
    for reservation in normalized_reservations:
        reserved_abs = abs(reservation.reserved_qty)
        attributed_abs = min(remaining_fill_abs, reserved_abs)
        remaining_fill_abs -= attributed_abs
        allocations.append(
            AttributedFill(
                strategy_id=reservation.strategy_id,
                attributed_qty=attributed_abs * fill_sign,
                remaining_reservation_qty=(reserved_abs - attributed_abs) * fill_sign,
                reservation_seq=reservation.reservation_seq,
            )
        )

    return SharedNettingFillAttribution(
        controller_scope_id=scope_id,
        fill_qty=normalized_fill_qty,
        allocations=tuple(allocations),
        unattributed_qty=remaining_fill_abs * fill_sign,
    )


def _coerce_decimal(value: Decimal | str | int | float, field_name: str) -> Decimal:
    try:
        return Decimal(str(value).strip())
    except (AttributeError, InvalidOperation) as exc:
        raise ValueError(f"`{field_name}` must be a decimal-compatible value") from exc


def _decimal_sign(value: Decimal) -> int:
    if value > 0:
        return 1
    if value < 0:
        return -1
    return 0


def _required_text(value: str, field_name: str) -> str:
    text = str(value).strip()
    if not text:
        raise ValueError(f"`{field_name}` must be a non-empty string")
    return text


__all__ = (
    "AttributedFill",
    "AttributionReservation",
    "SharedNettingFillAttribution",
    "allocate_shared_netting_fill",
)
