from __future__ import annotations

import sys
from typing import Any

from flux.strategies.makerv3 import publisher as makerv3_publisher_mod


if __name__ == "flux.strategies.shared.alerts":
    sys.modules.setdefault(
        "nautilus_trader.flux.strategies.shared.alerts",
        sys.modules[__name__],
    )
elif __name__ == "nautilus_trader.flux.strategies.shared.alerts":
    sys.modules.setdefault("flux.strategies.shared.alerts", sys.modules[__name__])


def publish_actionable_alert(
    strategy: Any,
    *,
    alert_key: str,
    message: str,
    level: str = "warning",
    reason_code: str | None = None,
    cooldown_ms: int = 0,
    transition: str | None = None,
    now_ns: int | None = None,
    **extra_fields: Any,
) -> bool:
    return makerv3_publisher_mod.publish_actionable_alert(
        strategy,
        alert_key=alert_key,
        message=message,
        level=level,
        reason_code=reason_code,
        cooldown_ms=cooldown_ms,
        transition=transition,
        now_ns=now_ns,
        **extra_fields,
    )


def publish_alert(
    strategy: Any,
    message: str,
    level: str = "warning",
    *,
    ts_ns: int | None = None,
    alert_key: str | None = None,
    reason_code: str | None = None,
    actionable: bool | None = None,
    **extra_fields: Any,
) -> None:
    makerv3_publisher_mod.publish_alert(
        strategy,
        message,
        level,
        ts_ns=ts_ns,
        alert_key=alert_key,
        reason_code=reason_code,
        actionable=actionable,
        **extra_fields,
    )


__all__ = [
    "publish_actionable_alert",
    "publish_alert",
]
