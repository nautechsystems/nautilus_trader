from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
import sys
from typing import Callable
from typing import Any

from flux.strategies.makerv3.publisher import decimal_to_json_str


if __name__ == "flux.strategies.shared.equities_arb.hedging":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.equities_arb.hedging",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.equities_arb.hedging":
    sys.modules.setdefault("flux.strategies.shared.equities_arb.hedging", sys.modules[__name__])


@dataclass(frozen=True, slots=True)
class HedgeOrderIntent:
    instrument_id: str
    side: str
    qty: Decimal
    limit_price: Decimal
    route: str = "SMART"
    time_in_force: str = "IOC"
    outside_rth: bool = False
    include_overnight: bool = False
    cancel_after_ms: int | None = None


@dataclass(frozen=True, slots=True)
class PendingHedgeState:
    fill_id: str
    side: str
    requested_qty: Decimal
    remaining_qty: Decimal
    limit_price: Decimal
    route: str
    time_in_force: str
    outside_rth: bool
    include_overnight: bool = False
    cancel_after_ms: int | None = None
    order_id: str | None = None


@dataclass(frozen=True, slots=True)
class HedgeBacklogState:
    fill_id: str
    side: str
    requested_qty: Decimal
    blocked_reason: str
    fill_ts_ms: int
    maker_fee_bps: Decimal


def _normalized_route(configured_route: str | None) -> str:
    route = str(configured_route or "").strip().upper()
    return route or "SMART"


def build_pending_hedge_payload(
    pending_hedge: PendingHedgeState | None,
    *,
    hedge_instrument_id: Any,
    decimal_to_json: Callable[[Decimal], Any] = decimal_to_json_str,
) -> dict[str, Any] | None:
    if pending_hedge is None:
        return None
    return {
        "client_order_id": pending_hedge.order_id,
        "instrument_id": str(hedge_instrument_id),
        "route": pending_hedge.route,
        "side": pending_hedge.side,
        "time_in_force": pending_hedge.time_in_force,
        "outside_rth": pending_hedge.outside_rth,
        "include_overnight": pending_hedge.include_overnight,
        "cancel_after_ms": pending_hedge.cancel_after_ms,
        "remaining_qty": decimal_to_json(pending_hedge.remaining_qty),
    }


def build_hedge_backlog_payload(
    hedge_backlog: HedgeBacklogState | None,
    *,
    decimal_to_json: Callable[[Decimal], Any] = decimal_to_json_str,
) -> dict[str, Any] | None:
    if hedge_backlog is None:
        return None
    return {
        "fill_id": hedge_backlog.fill_id,
        "side": hedge_backlog.side,
        "requested_qty": decimal_to_json(hedge_backlog.requested_qty),
        "blocked_reason": hedge_backlog.blocked_reason,
        "fill_ts_ms": hedge_backlog.fill_ts_ms,
        "maker_fee_bps": decimal_to_json(hedge_backlog.maker_fee_bps),
    }


def build_hedge_policy_payload(
    *,
    configured_route: str | None,
    outside_rth_enabled: bool,
    is_regular_session: bool,
    hedge_mode: str,
) -> dict[str, Any]:
    normalized_mode = str(hedge_mode).strip().lower() or "maker_hedge"
    if not is_regular_session:
        if normalized_mode == "take_take":
            return {
                "route": "SMART",
                "time_in_force": "DAY",
                "outside_rth": True,
                "include_overnight": True,
                "cancel_after_ms": 5_000,
            }
        return {
            "route": "SMART",
            "time_in_force": "IOC",
            "outside_rth": True,
            "include_overnight": True,
            "cancel_after_ms": None,
        }

    return {
        "route": _normalized_route(configured_route),
        "time_in_force": "IOC",
        "outside_rth": bool(outside_rth_enabled),
        "include_overnight": False,
        "cancel_after_ms": None,
    }


__all__ = [
    "HedgeBacklogState",
    "HedgeOrderIntent",
    "PendingHedgeState",
    "build_hedge_backlog_payload",
    "build_hedge_policy_payload",
    "build_pending_hedge_payload",
]
