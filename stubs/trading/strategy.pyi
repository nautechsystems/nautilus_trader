from typing import Any

from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.trading.config import ImportableStrategyConfig
from nautilus_trader.trading.config import StrategyConfig
from stubs.cache.base import CacheFacade
from stubs.cache.cache import Cache
from stubs.common.actor import Actor
from stubs.common.component import Clock
from stubs.common.component import MessageBus
from stubs.common.component import TimeEvent
from stubs.common.factories import OrderFactory
from stubs.core.message import Event
from stubs.model.events.order import OrderAccepted
from stubs.model.events.order import OrderCanceled
from stubs.model.events.order import OrderCancelRejected
from stubs.model.events.order import OrderDenied
from stubs.model.events.order import OrderEmulated
from stubs.model.events.order import OrderEvent
from stubs.model.events.order import OrderExpired
from stubs.model.events.order import OrderFilled
from stubs.model.events.order import OrderInitialized
from stubs.model.events.order import OrderModifyRejected
from stubs.model.events.order import OrderPendingCancel
from stubs.model.events.order import OrderPendingUpdate
from stubs.model.events.order import OrderRejected
from stubs.model.events.order import OrderReleased
from stubs.model.events.order import OrderSubmitted
from stubs.model.events.order import OrderTriggered
from stubs.model.events.order import OrderUpdated
from stubs.model.events.position import PositionChanged
from stubs.model.events.position import PositionClosed
from stubs.model.events.position import PositionEvent
from stubs.model.events.position import PositionOpened
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.orders.list import OrderList
from stubs.model.position import Position
from stubs.portfolio.base import PortfolioFacade

class Strategy(Actor):

    id: StrategyId
    order_id_tag: str
    use_uuid_client_order_ids: bool
    use_hyphens_in_client_order_ids: bool
    config: StrategyConfig
    oms_type: OmsType
    external_order_claims: list[InstrumentId]
    manage_contingent_orders: bool
    manage_gtd_expiry: bool
    clock: Clock
    cache: Cache
    portfolio: PortfolioFacade
    order_factory: OrderFactory

    def __init__(self, config: StrategyConfig | None = None): ...
    def _parse_external_order_claims(
        self,
        config_claims: list[str] | None,
    ) -> list[InstrumentId]: ...
    def to_importable_config(self) -> ImportableStrategyConfig: ...
    def on_start(self) -> None: ...
    def on_stop(self) -> None: ...
    def on_resume(self) -> None: ...
    def on_reset(self) -> None: ...
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
    def _start(self) -> None: ...
    def _reset(self) -> None: ...
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
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def submit_order_list(
        self,
        order_list: OrderList,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def modify_order(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def cancel_order(self, order: Order, client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None: ...
    def cancel_orders(self, orders: list[Order], client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None: ...
    def cancel_all_orders(
        self,
        instrument_id: InstrumentId,
        order_side: OrderSide = ...,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def close_position(
        self,
        position: Position,
        client_id: ClientId | None = None,
        tags: list[str] | None = None,
        time_in_force: TimeInForce = ...,
        reduce_only: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def close_all_positions(
        self,
        instrument_id: InstrumentId,
        position_side: PositionSide = ...,
        client_id: ClientId | None = None,
        tags: list[str] | None = None,
        time_in_force: TimeInForce = ...,
        reduce_only: bool = True,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def query_order(self, order: Order, client_id: ClientId | None = None, params: dict[str, Any] | None = None) -> None: ...
    def cancel_gtd_expiry(self, order: Order) -> None: ...
    def _expire_gtd_order(self, event: TimeEvent) -> None: ...
    def handle_event(self, event: Event) -> None: ...

