from typing import List

from nautilus_trader.model.identifiers import InstrumentId, OrderListId, StrategyId
from nautilus_trader.model.orders.base import Order

class OrderList:
    id: OrderListId
    instrument_id: InstrumentId
    strategy_id: StrategyId
    orders: List[Order]
    first: Order
    ts_init: int

    def __init__(
        self,
        order_list_id: OrderListId,
        orders: List[Order],
    ) -> None: ...
    def __eq__(self, other: OrderList) -> bool: ...
    def __hash__(self) -> int: ...
    def __len__(self) -> int: ...
    def __repr__(self) -> str: ...
