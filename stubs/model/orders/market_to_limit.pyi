import datetime as dt

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
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order

class MarketToLimitOrder(Order):
    """
    Represents a `Market-To-Limit` (MTL) order.

    A Market-to-Limit (MTL) order is submitted as a market order to execute at
    the current best market price. If the order is only partially filled, the
    remainder of the order is canceled and re-submitted as a Limit order with
    the limit price equal to the price at which the filled portion of the order
    executed.

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
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        UNIX timestamp (nanoseconds) when the order will expire.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
    display_qty : Quantity, optional
        The quantity of the limit order to display on the public book (iceberg).
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
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/mtl.php
    """

    price: Price
    expire_time_ns: int
    display_qty: Quantity | None

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
        expire_time_ns: int = 0,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        contingency_type: ContingencyType = ...,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, object] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: list[str] | None = None,
    ) -> None: ...
    @property
    def expire_time(self) -> dt.datetime | None:
        """
        Return the expire time for the order (UTC).

        Returns
        -------
        datetime or ``None``

        """
        ...
    def info(self) -> str:
        """
        Return a summary description of the order.

        Returns
        -------
        str

        """
        ...
    def to_dict(self) -> dict[str, object]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def create(init: OrderInitialized) -> MarketToLimitOrder: ...
