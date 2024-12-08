from typing import List, Optional

from nautilus_trader.cache.base import CacheFacade
from nautilus_trader.common.actor import Actor
from nautilus_trader.common.component import Clock, MessageBus
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.model import OmsType, OrderSide, PositionSide
from nautilus_trader.execution.manager import OrderManager
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
    InstrumentId,
    PositionId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.list import OrderList
from nautilus_trader.model.position import Position
from nautilus_trader.portfolio.base import PortfolioFacade
from nautilus_trader.trading.config import ImportableStrategyConfig, StrategyConfig

class Strategy(Actor):
    order_factory: OrderFactory
    order_id_tag: str
    oms_type: OmsType
    external_order_claims: List[InstrumentId]
    manage_contingent_orders: bool
    manage_gtd_expiry: bool
    _manager: OrderManager

    def __init__(self, config: Optional[StrategyConfig] = None) -> None: ...
    def to_importable_config(self) -> ImportableStrategyConfig: ...
    def register(
        self,
        trader_id: TraderId,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None: ...
    def change_id(self, strategy_id: StrategyId) -> None: ...
    def change_order_id_tag(self, order_id_tag: str) -> None: ...
    def on_start(self) -> None: ...
    def on_stop(self) -> None: ...
    def on_resume(self) -> None: ...
    def on_reset(self) -> None: ...
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
    def submit_order(
        self,
        order: Order,
        position_id: Optional[PositionId] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def submit_order_list(
        self,
        order_list: OrderList,
        position_id: Optional[PositionId] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def modify_order(
        self,
        order: Order,
        quantity: Optional[Quantity] = None,
        price: Optional[Price] = None,
        trigger_price: Optional[Price] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def cancel_order(
        self,
        order: Order,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def cancel_orders(
        self,
        orders: List[Order],
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def cancel_all_orders(
        self,
        instrument_id: InstrumentId,
        order_side: Optional[OrderSide] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def close_position(
        self,
        position: Position,
        client_id: Optional[ClientId] = None,
        tags: Optional[List[str]] = None,
        reduce_only: bool = True,
    ) -> None: ...
    def close_all_positions(
        self,
        instrument_id: InstrumentId,
        position_side: Optional[PositionSide] = None,
        client_id: Optional[ClientId] = None,
        tags: Optional[List[str]] = None,
        reduce_only: bool = True,
    ) -> None: ...
    def query_order(
        self,
        order: Order,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    def cancel_gtd_expiry(self, order: Order) -> None: ...
