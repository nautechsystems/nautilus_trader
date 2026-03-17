from __future__ import annotations

from dataclasses import dataclass
from decimal import Decimal
from typing import Literal
import sys


FeedState = Literal["ok", "degraded", "down", "unknown"]
QuoteState = Literal["fresh", "old", "missing"]
AlertLevel = Literal["warning", "critical"]


@dataclass(frozen=True, slots=True)
class QuoteHealth:
    leg_role: str
    feed_state: FeedState
    quote_state: QuoteState
    quote_age_ms: int | None
    usable_for_pricing: bool
    usable_for_hedging: bool
    reason_code: str | None = None
    alert_level: AlertLevel | None = None


def _normalize_feed_state(
    *,
    transport_connected: bool | None,
    subscription_healthy: bool | None,
) -> FeedState:
    if transport_connected is False or subscription_healthy is False:
        return "down"
    if transport_connected is True and subscription_healthy is True:
        return "ok"
    if transport_connected is None and subscription_healthy is None:
        return "unknown"
    return "degraded"


def evaluate_quote_health(
    *,
    leg_role: str,
    bid: Decimal | None,
    ask: Decimal | None,
    quote_age_ms: int | None,
    max_quote_age_ms: int,
    transport_connected: bool | None = None,
    subscription_healthy: bool | None = None,
) -> QuoteHealth:
    feed_state = _normalize_feed_state(
        transport_connected=transport_connected,
        subscription_healthy=subscription_healthy,
    )
    has_quote = bid is not None and ask is not None and ask > bid
    normalized_age_ms = max(0, int(quote_age_ms)) if quote_age_ms is not None else None

    if has_quote:
        if normalized_age_ms is None:
            quote_state: QuoteState = "missing"
        elif normalized_age_ms > max(0, int(max_quote_age_ms)):
            quote_state = "old"
        else:
            quote_state = "fresh"
    else:
        quote_state = "missing"

    reason_code: str | None = None
    alert_level: AlertLevel | None = None
    if feed_state == "down":
        reason_code = f"{leg_role}_feed_down"
        alert_level = "critical"
    elif feed_state in {"degraded", "unknown"}:
        reason_code = f"{leg_role}_feed_{feed_state}"
        alert_level = "warning"
    elif quote_state == "missing":
        reason_code = f"{leg_role}_quote_missing"
        alert_level = "warning"
    elif quote_state == "old":
        reason_code = f"{leg_role}_quote_old"
        alert_level = "warning"

    usable = feed_state == "ok" and quote_state == "fresh"
    return QuoteHealth(
        leg_role=leg_role,
        feed_state=feed_state,
        quote_state=quote_state,
        quote_age_ms=normalized_age_ms,
        usable_for_pricing=usable,
        usable_for_hedging=usable,
        reason_code=reason_code,
        alert_level=alert_level,
    )


if __name__ == "flux.strategies.shared.quote_health":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.quote_health",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.quote_health":
    sys.modules.setdefault("flux.strategies.shared.quote_health", sys.modules[__name__])
