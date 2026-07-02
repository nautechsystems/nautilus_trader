"""Base strategy and context types for Polymarket v1 research backtests."""

from __future__ import annotations

from abc import ABC
from dataclasses import dataclass
from decimal import Decimal
from typing import TYPE_CHECKING, Any, Literal

if TYPE_CHECKING:
    from polymarket.backtest_v1 import BacktestContextV1, L2BookViewV1
    from polymarket.models import L2ReplayStepV1


OrderSideV1 = Literal["BUY", "SELL"]


@dataclass(frozen=True, slots=True)
class StrategyOrderRequestV1:
    side: OrderSideV1
    quantity: Decimal
    price: Decimal | None = None
    order_type: Literal["market", "limit"] = "market"
    label: str = ""


class BasePolymarketStrategyV1(ABC):
    """Minimal v1 strategy interface."""

    def __init__(self, **params: Any) -> None:
        self.params = params

    def on_start(self, context: BacktestContextV1) -> None:
        """Called once before replay starts."""

    def on_replay_step(
        self,
        step: L2ReplayStepV1,
        book: L2BookViewV1,
        context: BacktestContextV1,
    ) -> None:
        """Called after the current replay step has updated the L2 book."""

    def on_finish(self, context: BacktestContextV1) -> None:
        """Called once after replay ends."""

