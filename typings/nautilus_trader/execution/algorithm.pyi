from datetime import datetime
from typing import Dict, List, Optional, Set

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.component import Clock, MessageBus
from nautilus_trader.core.model import TimeInForce, TriggerType
from nautilus_trader.execution.config import (
    ExecAlgorithmConfig,
    ImportableExecAlgorithmConfig,
)
from nautilus_trader.execution.messages import (
    TradingCommand,
)
from nautilus_trader.model.events.order import (
    OrderAccepted,
    OrderCanceled,
    OrderCancelRejected,
    OrderDenied,
    OrderEmulated,
    OrderEvent,
    OrderExpired,
    OrderFilled,
    OrderInitialized,
    OrderModifyRejected,
    OrderPendingCancel,
    OrderPendingUpdate,
    OrderRejected,
    OrderReleased,
    OrderSubmitted,
    OrderTriggered,
    OrderUpdated,
)
from nautilus_trader.model.events.position import (
    PositionChanged,
    PositionClosed,
    PositionEvent,
    PositionOpened,
)
from nautilus_trader.model.identifiers import (
    ClientId,
    ClientOrderId,
    ExecAlgorithmId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.limit import LimitOrder
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.model.orders.market import MarketOrder
from nautilus_trader.model.orders.market_to_limit import MarketToLimitOrder
from nautilus_trader.portfolio.base import PortfolioFacade

class ExecAlgorithm(Actor):
    id: ExecAlgorithmId
    config: ExecAlgorithmConfig  # type: ignore  # bug where ExecAlgorithmConfig doesn't inherit from ActorConfig
    trader_id: Optional[TraderId]
    portfolio: PortfolioFacade
    _exec_spawn_ids: Dict[ClientOrderId, int]
    _subscribed_strategies: Set[StrategyId]

    def __init__(self, config: Optional[ExecAlgorithmConfig] = None) -> None: ...
    def to_importable_config(self) -> ImportableExecAlgorithmConfig: ...
    def register(
        self,
        trader_id: TraderId,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None: ...
    def execute(self, command: TradingCommand) -> None: ...
    def on_order(self, order: Order) -> None: ...
    def on_order_list(self, order_list: OrderList) -> None: ...
    def on_order_event(self, event: OrderEvent) -> None: ...
    def on_order_initialized(self, event: OrderInitialized) -> None: ...
    def on_order_denied(self, event: OrderDenied) -> None: ...
    def on_order_emulated(self, event: OrderEmulated) -> None: ...
    def on_order_released(self, event: OrderReleased) -> None: ...
    def on_order_submitted(self, event: OrderSubmitted) -> None: ...
    def on_order_rejected(self, event: OrderRejected) -> None: ...
    def on_order_accepted(self, event: OrderAccepted) -> None: ...
    def on_order_canceled(self, event: OrderCanceled) -> None: ...
    def on_order_expired(self, event: OrderExpired) -> None: ...
    def on_order_triggered(self, event: OrderTriggered) -> None: ...
    def on_order_pending_update(self, event: OrderPendingUpdate) -> None: ...
    def on_order_pending_cancel(self, event: OrderPendingCancel) -> None: ...
    def on_order_modify_rejected(self, event: OrderModifyRejected) -> None: ...
    def on_order_cancel_rejected(self, event: OrderCancelRejected) -> None: ...
    def on_order_updated(self, event: OrderUpdated) -> None: ...
    def on_order_filled(self, event: OrderFilled) -> None: ...
    def on_position_event(self, event: PositionEvent) -> None: ...
    def on_position_opened(self, event: PositionOpened) -> None: ...
    def on_position_changed(self, event: PositionChanged) -> None: ...
    def on_position_closed(self, event: PositionClosed) -> None: ...
    def spawn_market(
        self,
        primary: Order,
        quantity: Quantity,
        time_in_force: TimeInForce = TimeInForce.GTC,
        reduce_only: bool = False,
        tags: Optional[List[str]] = None,
        reduce_primary: bool = True,
    ) -> MarketOrder: ...
    def spawn_limit(
        self,
        primary: Order,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        post_only: bool = False,
        reduce_only: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        tags: Optional[List[str]] = None,
        reduce_primary: bool = True,
    ) -> LimitOrder: ...
    def spawn_market_to_limit(
        self,
        primary: Order,
        quantity: Quantity,
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time: Optional[datetime] = None,
        reduce_only: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        tags: Optional[List[str]] = None,
        reduce_primary: bool = True,
    ) -> MarketToLimitOrder: ...
    def submit_order(self, order: Order) -> None: ...
    def modify_order(
        self,
        order: Order,
        quantity: Optional[Quantity] = None,
        price: Optional[Price] = None,
        trigger_price: Optional[Price] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def modify_order_in_place(
        self,
        order: Order,
        quantity: Optional[Quantity] = None,
        price: Optional[Price] = None,
        trigger_price: Optional[Price] = None,
    ) -> None: ...
    def cancel_order(
        self, order: Order, client_id: Optional[ClientId] = None
    ) -> None: ...
