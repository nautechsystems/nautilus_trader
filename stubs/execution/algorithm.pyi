import datetime as dt

from nautilus_trader.execution.config import ExecAlgorithmConfig
from nautilus_trader.execution.config import ImportableExecAlgorithmConfig
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from stubs.cache.base import CacheFacade
from stubs.common.actor import Actor
from stubs.common.component import Clock
from stubs.common.component import MessageBus
from stubs.execution.messages import TradingCommand
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
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.orders.limit import LimitOrder
from stubs.model.orders.list import OrderList
from stubs.model.orders.market import MarketOrder
from stubs.model.orders.market_to_limit import MarketToLimitOrder
from stubs.portfolio.base import PortfolioFacade

class ExecAlgorithm(Actor):

    id: ExecAlgorithmId
    portfolio: PortfolioFacade
    config: ExecAlgorithmConfig

    _log_events: bool
    _log_commands: bool
    _exec_spawn_ids: dict[ClientOrderId, int]
    _subscribed_strategies: set[StrategyId]

    def __init__(self, config: ExecAlgorithmConfig | None = None) -> None: ...
    def to_importable_config(self) -> ImportableExecAlgorithmConfig: ...
    def register(
        self,
        trader_id: TraderId,
        portfolio: PortfolioFacade,
        msgbus: MessageBus,
        cache: CacheFacade,
        clock: Clock,
    ) -> None: ...
    def _reset(self) -> None: ...
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
        time_in_force: TimeInForce = ...,
        reduce_only: bool = False,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> MarketOrder: ...
    def spawn_limit(
        self,
        primary: Order,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce = ...,
        expire_time: dt.datetime | None = None,
        post_only: bool = False,
        reduce_only: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> LimitOrder: ...
    def spawn_market_to_limit(
        self,
        primary: Order,
        quantity: Quantity,
        time_in_force: TimeInForce = ...,
        expire_time: dt.datetime | None = None,
        reduce_only: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        tags: list[str] | None = None,
        reduce_primary: bool = True,
    ) -> MarketToLimitOrder: ...
    def submit_order(self, order: Order) -> None: ...
    def modify_order(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
        client_id: ClientId | None = None,
    ) -> None: ...
    def modify_order_in_place(
        self,
        order: Order,
        quantity: Quantity | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
    ) -> None: ...
    def cancel_order(self, order: Order, client_id: ClientId | None = None) -> None: ...

