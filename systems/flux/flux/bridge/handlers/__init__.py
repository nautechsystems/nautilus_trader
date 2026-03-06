from __future__ import annotations

from flux.bridge.handlers.alerts import transform_alert
from flux.bridge.handlers.balances import transform_balances
from flux.bridge.handlers.events import transform_event
from flux.bridge.handlers.fv import transform_fv
from flux.bridge.handlers.market_bbo import transform_market_bbo
from flux.bridge.handlers.state import transform_state
from flux.bridge.handlers.trades import transform_trade
from flux.bridge.handlers.types import HandlerFn


def default_topic_handlers() -> dict[str, HandlerFn]:
    return {
        "state": transform_state,
        "event": transform_event,
        "trade": transform_trade,
        "alert": transform_alert,
        "market_bbo": transform_market_bbo,
        "fv": transform_fv,
        "balances": transform_balances,
    }


__all__ = ["default_topic_handlers"]
