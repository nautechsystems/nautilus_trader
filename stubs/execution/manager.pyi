from collections.abc import Callable

from stubs.cache.cache import Cache
from stubs.common.component import Clock
from stubs.common.component import Logger
from stubs.common.component import MessageBus
from stubs.core.message import Event
from stubs.execution.messages import SubmitOrder
from stubs.execution.messages import TradingCommand
from stubs.model.events.order import OrderCanceled
from stubs.model.events.order import OrderEvent
from stubs.model.events.order import OrderExpired
from stubs.model.events.order import OrderFilled
from stubs.model.events.order import OrderRejected
from stubs.model.events.order import OrderUpdated
from stubs.model.events.position import PositionEvent
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import PositionId
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order

class OrderManager:

    active_local: bool
    debug: bool
    log_events: bool
    log_commands: bool

    _clock: Clock
    _log: Logger
    _msgbus: MessageBus
    _cache: Cache
    _submit_order_handler: Callable[[SubmitOrder], None] | None
    _cancel_order_handler: Callable[[Order], None] | None
    _modify_order_handler: Callable[[Order, Quantity], None] | None
    _submit_order_commands: dict[ClientOrderId, SubmitOrder]

    def __init__(
        self,
        clock: Clock,
        msgbus: MessageBus,
        cache: Cache,
        component_name: str,
        active_local: bool,
        submit_order_handler: Callable[[SubmitOrder], None] = ...,
        cancel_order_handler: Callable[[Order], None] = ...,
        modify_order_handler: Callable[[Order, Quantity], None] = ...,
        debug: bool = False,
        log_events: bool = True,
        log_commands: bool = True,
    ) -> None: ...
    def get_submit_order_commands(self) -> dict[ClientOrderId, SubmitOrder]: ...
    def cache_submit_order_command(self, command: SubmitOrder) -> None: ...
    def pop_submit_order_command(self, client_order_id: ClientOrderId) -> SubmitOrder | None: ...
    def reset(self) -> None: ...
    def cancel_order(self, order: Order) -> None: ...
    def modify_order_quantity(self, order: Order, new_quantity: Quantity) -> None: ...
    def create_new_submit_order(
        self,
        order: Order,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
    ) -> None: ...
    def should_manage_order(self, order: Order) -> bool: ...
    def handle_event(self, event: Event) -> None: ...
    def handle_order_rejected(self, rejected: OrderRejected) -> None: ...
    def handle_order_canceled(self, canceled: OrderCanceled) -> None: ...
    def handle_order_expired(self, expired: OrderExpired) -> None: ...
    def handle_order_updated(self, updated: OrderUpdated) -> None: ...
    def handle_order_filled(self, filled: OrderFilled) -> None: ...
    def handle_contingencies(self, order: Order) -> None: ...
    def handle_contingencies_update(self, order: Order) -> None: ...
    def handle_position_event(self, event: PositionEvent) -> None: ...
    def send_emulator_command(self, command: TradingCommand) -> None: ...
    def send_algo_command(self, command: TradingCommand, exec_algorithm_id: ExecAlgorithmId) -> None: ...
    def send_risk_command(self, command: TradingCommand) -> None: ...
    def send_exec_command(self, command: TradingCommand) -> None: ...
    def send_risk_event(self, event: OrderEvent) -> None: ...
    def send_exec_event(self, event: OrderEvent) -> None: ...

