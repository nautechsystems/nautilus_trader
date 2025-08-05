from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import StrategyId
from stubs.model.orders.base import Order

class OrderList:
    """
    Represents a list of bulk or related contingent orders.

    All orders must be for the same instrument ID.

    Parameters
    ----------
    order_list_id : OrderListId
        The order list ID.
    orders : list[Order]
        The contained orders list.

    Raises
    ------
    ValueError
        If `orders` is empty.
    ValueError
        If `orders` contains a type other than `Order`.
    ValueError
        If orders contain different instrument IDs (must all be the same instrument).

    """

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
