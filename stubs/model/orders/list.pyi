from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import StrategyId
from stubs.model.orders.base import Order

class OrderList:

    id: OrderListId
    instrument_id: InstrumentId
    strategy_id: StrategyId
    orders: list[Order]
    first: Order
    ts_init: int

    def __init__(
        self,
        order_list_id: OrderListId,
        orders: list[Order],
    ) -> None: ...
    def __eq__(self, other: OrderList) -> bool: ...
    def __hash__(self) -> int: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...
