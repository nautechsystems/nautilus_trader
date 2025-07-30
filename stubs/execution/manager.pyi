from collections.abc import Callable

from nautilus_trader.core.nautilus_pyo3 import ClientId
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderCanceled
from nautilus_trader.core.nautilus_pyo3 import OrderEvent
from nautilus_trader.core.nautilus_pyo3 import OrderExpired
from nautilus_trader.core.nautilus_pyo3 import OrderFilled
from nautilus_trader.core.nautilus_pyo3 import OrderRejected
from nautilus_trader.core.nautilus_pyo3 import OrderUpdated
from nautilus_trader.core.nautilus_pyo3 import PositionId
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import TradingCommand
from stubs.cache.cache import Cache
from stubs.common.component import Clock
from stubs.common.component import Logger
from stubs.execution.messages import SubmitOrder

class OrderManager:
    """
    Provides a generic order execution manager.

    Parameters
    ----------
    clock : Clock
        The clock for the order manager.
    msgbus : MessageBus
        The message bus for the order manager.
    cache : Cache
        The cache for the order manager.
    component_name : str
        The component name for the order manager.
    active_local : str
        If the manager is for active local orders.
    submit_order_handler : Callable[[SubmitOrder], None], optional
        The handler to call when submitting orders.
    cancel_order_handler : Callable[[Order], None], optional
        The handler to call when canceling orders.
    modify_order_handler : Callable[[Order, Quantity], None], optional
        The handler to call when modifying orders (limited to modifying quantity).
    debug : bool, default False
        If debug mode is active (will provide extra debug logging).

    Raises
    ------
    TypeError
        If `submit_order_handler` is not ``None`` and not of type `Callable`.
    TypeError
        If `cancel_order_handler` is not ``None`` and not of type `Callable`.
    TypeError
        If `modify_order_handler` is not ``None`` and not of type `Callable`.
    """

    active_local: bool
    debug: bool
    log_events: bool
    log_commands: bool

    _clock: Clock
    _log: Logger
    _msgbus: MessageBus
    _cache: Cache
    _submit_order_handler: Callable[[SubmitOrder], None]
    _cancel_order_handler: Callable[[Order], None]
    _modify_order_handler: Callable[[Order, Quantity], None]
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
    def get_submit_order_commands(self) -> dict[ClientOrderId, TradingCommand]: ...
    def cache_submit_order_command(self, command) -> None: ...
    def pop_submit_order_command(self, client_order_id: ClientOrderId): ...
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
    def handle_event(self, event) -> None: ...
    def handle_order_rejected(self, rejected: OrderRejected) -> None: ...
    def handle_order_canceled(self, canceled: OrderCanceled) -> None: ...
    def handle_order_expired(self, expired: OrderExpired) -> None: ...
    def handle_order_updated(self, updated: OrderUpdated) -> None: ...
    def handle_order_filled(self, filled: OrderFilled) -> None: ...
    def handle_contingencies(self, order: Order) -> None: ...
    def handle_contingencies_update(self, order: Order) -> None: ...
    def handle_position_event(self, event) -> None: ...
    def send_emulator_command(self, command: TradingCommand) -> None: ...
    def send_algo_command(self, command: TradingCommand, exec_algorithm_id) -> None: ...
    def send_risk_command(self, command: TradingCommand) -> None: ...
    def send_exec_command(self, command: TradingCommand) -> None: ...
    def send_risk_event(self, event: OrderEvent) -> None: ...
    def send_exec_event(self, event: OrderEvent) -> None: ...