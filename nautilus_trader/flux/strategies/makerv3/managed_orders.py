"""Handle collection, tracking, and cancellation of strategy-managed orders."""

from __future__ import annotations

from collections.abc import Callable
from dataclasses import dataclass
from typing import Any


CANCELLATION_SAFETY_INVARIANT = (
    "Cancel only strategy-managed orders by default; "
    "instrument-wide cancel requires explicit opt-in."
)


@dataclass(frozen=True)
class CancelManagedQuotesResult:
    """Represent the outcome of a managed-quote cancellation attempt."""

    should_cancel: bool
    tracked_count: int
    cache_count: int
    cancel_attempts: int
    cancel_exceptions: int
    cancel_success: int
    cancel_all_instrument: bool
    cancel_all_attempted: bool
    cancel_all_exceptions: int


def _managed_order_dedupe_key(order: Any) -> tuple[Any, ...]:
    client_order_id = str(getattr(order, "client_order_id", "") or "")
    venue_order_id = str(getattr(order, "venue_order_id", "") or "")
    if client_order_id:
        return ("client", client_order_id)
    if venue_order_id:
        return ("venue", venue_order_id)
    return (
        "shape",
        str(getattr(order, "side", "")),
        str(getattr(order, "price", "")),
        str(getattr(order, "quantity", "")),
        int(getattr(order, "ts_init", 0) or 0),
    )


def collect_managed_orders(*, cache: Any, instrument_id: Any, strategy_id: Any) -> list[Any]:
    """Collect currently open/inflight managed orders with deduplication."""
    orders: list[Any] = []
    seen_order_keys: set[tuple[Any, ...]] = set()
    sources: list[list[Any]] = []
    for fetch_name in ("orders_open", "orders_inflight"):
        fetch = getattr(cache, fetch_name, None)
        if not callable(fetch):
            continue
        try:
            rows = fetch(
                instrument_id=instrument_id,
                strategy_id=strategy_id,
            )
        except Exception:
            rows = []
        if rows is None:
            rows = []
        sources.append(list(rows))

    for source in sources:
        for order in source:
            is_closed = getattr(order, "is_closed", False)
            if callable(is_closed):
                try:
                    is_closed = bool(is_closed())
                except Exception:
                    is_closed = False
            if bool(is_closed):
                continue
            dedupe_key = _managed_order_dedupe_key(order)
            if dedupe_key in seen_order_keys:
                continue
            seen_order_keys.add(dedupe_key)
            orders.append(order)
    return orders


def register_managed_order(tracked_ids: set[str], order: Any) -> str | None:
    """Register a managed order client ID and return it when present."""
    client_order_id = str(getattr(order, "client_order_id", "") or "")
    if not client_order_id:
        return None
    tracked_ids.add(client_order_id)
    return client_order_id


def reconcile_managed_order(tracked_ids: set[str], client_order_id: Any) -> bool:
    """Remove a managed order ID from tracking and return prior membership."""
    client_order_id_str = str(client_order_id or "")
    if not client_order_id_str:
        return False
    had_order = client_order_id_str in tracked_ids
    tracked_ids.discard(client_order_id_str)
    return had_order


def cancel_managed_quotes(
    *,
    reason: str,
    force: bool,
    tracked_ids: set[str],
    managed_orders: list[Any],
    maker_instrument_id: Any,
    cancel_order: Callable[[Any], None],
    cancel_all_orders: Callable[[Any], None] | None,
    cancel_all_instrument_orders: bool = False,
) -> CancelManagedQuotesResult:
    """Cancel managed orders and optionally cancel all instrument orders."""
    tracked_count = len(tracked_ids)
    cancel_all_instrument = bool(cancel_all_instrument_orders)
    should_cancel = bool(managed_orders or tracked_count > 0 or cancel_all_instrument)
    if not should_cancel:
        return CancelManagedQuotesResult(
            should_cancel=False,
            tracked_count=tracked_count,
            cache_count=0,
            cancel_attempts=0,
            cancel_exceptions=0,
            cancel_success=0,
            cancel_all_instrument=cancel_all_instrument,
            cancel_all_attempted=False,
            cancel_all_exceptions=0,
        )

    cancel_attempts = len(managed_orders)
    cancel_exceptions = 0
    for order in managed_orders:
        try:
            cancel_order(order)
        except Exception:
            cancel_exceptions += 1

    cancel_all_attempted = bool(cancel_all_instrument and cancel_all_orders is not None)
    cancel_all_exceptions = 0
    if cancel_all_attempted:
        try:
            cancel_all_orders(maker_instrument_id)
        except Exception:
            cancel_all_exceptions += 1

    if force and reason in {"on_stop", "quote_fail_circuit_breaker"}:
        tracked_ids.clear()
    elif not managed_orders:
        tracked_ids.clear()

    return CancelManagedQuotesResult(
        should_cancel=True,
        tracked_count=tracked_count,
        cache_count=len(managed_orders),
        cancel_attempts=cancel_attempts,
        cancel_exceptions=cancel_exceptions,
        cancel_success=max(0, cancel_attempts - cancel_exceptions),
        cancel_all_instrument=cancel_all_instrument,
        cancel_all_attempted=cancel_all_attempted,
        cancel_all_exceptions=cancel_all_exceptions,
    )


__all__ = [
    "CANCELLATION_SAFETY_INVARIANT",
    "CancelManagedQuotesResult",
    "cancel_managed_quotes",
    "collect_managed_orders",
    "reconcile_managed_order",
    "register_managed_order",
]
