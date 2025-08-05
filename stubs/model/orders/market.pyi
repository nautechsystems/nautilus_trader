from typing import Any

from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from stubs.core.uuid import UUID4
from stubs.model.events.order import OrderInitialized
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order

class MarketOrder(Order):
    """
    Represents a `Market` order.

    A Market order is an order to BUY (or SELL) at the market bid or offer price.
    A market order may increase the likelihood of a fill and the speed of
    execution, but unlike the Limit order - a Market order provides no price
    protection and may fill at a price far lower/higher than the top-of-book
    bid/ask.

    - A `Market-On-Open (MOO)` order can be represented using a time in force of ``AT_THE_OPEN``.
    - A `Market-On-Close (MOC)` order can be represented using a time in force of ``AT_THE_CLOSE``.

    Parameters
    ----------
    trader_id : TraderId
        The trader ID associated with the order.
    strategy_id : StrategyId
        The strategy ID associated with the order.
    instrument_id : InstrumentId
        The order instrument ID.
    client_order_id : ClientOrderId
        The client order ID.
    order_side : OrderSide {``BUY``, ``SELL``}
        The order side.
    quantity : Quantity
        The order quantity (> 0).
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``DAY``, ``AT_THE_OPEN``, ``AT_THE_CLOSE``}, default ``GTC``
        The order time in force.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
    contingency_type : ContingencyType, default ``NO_CONTINGENCY``
        The order contingency type.
    order_list_id : OrderListId, optional
        The order list ID associated with the order.
    linked_order_ids : list[ClientOrderId], optional
        The order linked client order ID(s).
    parent_order_id : ClientOrderId, optional
        The order parent client order ID.
    exec_algorithm_id : ExecAlgorithmId, optional
        The execution algorithm ID for the order.
    exec_algorithm_params : dict[str, Any], optional
        The execution algorithm parameters for the order.
    exec_spawn_id : ClientOrderId, optional
        The execution algorithm spawning primary client order ID.
    tags : list[str], optional
        The custom user tags for the order.

    Raises
    ------
    ValueError
        If `order_side` is ``NO_ORDER_SIDE``.
    ValueError
        If `quantity` is not positive (> 0).
    ValueError
        If `time_in_force` is ``GTD``.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/market.php
    """

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        init_id: UUID4,
        ts_init: int,
        time_in_force: TimeInForce = ...,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        contingency_type: ContingencyType = ...,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: list[str] | None = None,
    ) -> None: ...
    def info(self) -> str:
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
    @staticmethod
    def from_pyo3(pyo3_order: Any) -> MarketOrder: ...
    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
    @staticmethod
    def create(init: OrderInitialized) -> MarketOrder: ...
    @staticmethod
    def transform_py(order: Order, ts_init: int) -> MarketOrder: ...
