from __future__ import annotations

import sys
from dataclasses import dataclass
from datetime import UTC
from datetime import datetime
from datetime import time


try:
    from zoneinfo import ZoneInfo
except Exception:  # pragma: no cover - Python/runtime compatibility fallback
    ZoneInfo = None  # type: ignore[assignment]


US_EQUITIES_REGULAR_TZ = "America/New_York"
US_EQUITIES_REGULAR_START = time(9, 30)
US_EQUITIES_REGULAR_END = time(16, 0)


@dataclass(frozen=True, slots=True)
class IbkrHedgeOrderPolicy:
    route: str
    time_in_force: str
    outside_rth: bool
    include_overnight: bool
    cancel_after_ms: int | None = None


def _normalized_route(configured_route: str | None) -> str:
    route = str(configured_route or "").strip().upper()
    return route or "SMART"


def build_ibkr_hedge_order_policy(
    *,
    configured_route: str | None,
    outside_rth_enabled: bool,
    is_regular_session: bool,
    hedge_mode: str = "maker_hedge",
    overnight_cancel_after_ms: int = 5_000,
) -> IbkrHedgeOrderPolicy:
    _ = str(hedge_mode).strip().lower() or "maker_hedge"
    route = _normalized_route(configured_route)
    return IbkrHedgeOrderPolicy(
        route=route if is_regular_session else "SMART",
        time_in_force="IOC",
        outside_rth=bool(outside_rth_enabled) if is_regular_session else True,
        include_overnight=not is_regular_session,
        cancel_after_ms=None,
    )


def is_us_equities_regular_session(now_ms: int) -> bool:
    if ZoneInfo is None:
        return True
    local_dt = datetime.fromtimestamp(now_ms / 1000, tz=UTC).astimezone(
        ZoneInfo(US_EQUITIES_REGULAR_TZ),
    )
    if local_dt.weekday() >= 5:
        return False
    local_time = local_dt.timetz().replace(tzinfo=None)
    return US_EQUITIES_REGULAR_START <= local_time < US_EQUITIES_REGULAR_END


if __name__ == "flux.strategies.shared.ibkr_order_policy":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.ibkr_order_policy",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.ibkr_order_policy":
    sys.modules.setdefault("flux.strategies.shared.ibkr_order_policy", sys.modules[__name__])


__all__ = ("IbkrHedgeOrderPolicy", "build_ibkr_hedge_order_policy", "is_us_equities_regular_session")
