from nautilus_trader.core.nautilus_pyo3 import BatchCancelOrders
from nautilus_trader.core.nautilus_pyo3 import Cache
from nautilus_trader.core.nautilus_pyo3 import CancelAllOrders
from nautilus_trader.core.nautilus_pyo3 import CancelOrder
from nautilus_trader.core.nautilus_pyo3 import ExecutionClient
from nautilus_trader.core.nautilus_pyo3 import MessageBus
from nautilus_trader.core.nautilus_pyo3 import ModifyOrder
from nautilus_trader.core.nautilus_pyo3 import SimulatedExchange
from nautilus_trader.core.nautilus_pyo3 import SubmitOrder
from nautilus_trader.core.nautilus_pyo3 import SubmitOrderList
from nautilus_trader.core.nautilus_pyo3 import TestClock

class BacktestExecClient(ExecutionClient):
    """
    Provides an execution client for the `BacktestEngine`.

    Parameters
    ----------
    exchange : SimulatedExchange
        The simulated exchange for the backtest.
    msgbus : MessageBus
        The message bus for the client.
    cache : Cache
        The cache for the client.
    clock : TestClock
        The clock for the client.
    routing : bool
        If multi-venue routing is enabled for the client.
    frozen_account : bool
        If the backtest run account is frozen.
    """

    def __init__(
        self,
        exchange: SimulatedExchange,
        msgbus: MessageBus,
        cache: Cache,
        clock: TestClock,
        routing: bool = False,
        frozen_account: bool = False,
    ) -> None: ...
    def _start(self) -> None: ...
    def _stop(self) -> None: ...
    def submit_order(self, command: SubmitOrder) -> None: ...
    def submit_order_list(self, command: SubmitOrderList) -> None: ...
    def modify_order(self, command: ModifyOrder) -> None: ...
    def cancel_order(self, command: CancelOrder) -> None: ...
    def cancel_all_orders(self, command: CancelAllOrders) -> None: ...
    def batch_cancel_orders(self, command: BatchCancelOrders) -> None: ...
