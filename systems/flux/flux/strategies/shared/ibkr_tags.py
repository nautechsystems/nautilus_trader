from __future__ import annotations

from nautilus_trader.adapters.interactive_brokers.common import IBOrderTags


def build_ibkr_order_tags(
    *,
    outside_rth: bool = False,
    include_overnight: bool = False,
) -> list[str] | None:
    if not outside_rth and not include_overnight:
        return None
    return [IBOrderTags(outsideRth=outside_rth, includeOvernight=include_overnight).value]


__all__ = ("build_ibkr_order_tags",)
