from stubs.backtest.exchange import SimulatedExchange
from stubs.cache.cache import Cache
from stubs.common.component import MessageBus
from stubs.common.component import TestClock
from stubs.execution.client import ExecutionClient
from stubs.execution.messages import BatchCancelOrders
from stubs.execution.messages import CancelAllOrders
from stubs.execution.messages import CancelOrder
from stubs.execution.messages import ModifyOrder
from stubs.execution.messages import SubmitOrder
from stubs.execution.messages import SubmitOrderList

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
