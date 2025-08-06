from datetime import datetime
from typing import Any

from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from stubs.core.uuid import UUID4
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import OrderListId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order

class LimitIfTouchedOrder(Order):
    """
    Represents a `Limit-If-Touched` (LIT) conditional order.

    A Limit-If-Touched (LIT) is an order to BUY (or SELL) an instrument at a
    specified price or better, below (or above) the market. This order is held
    in the system until the trigger price is touched. A LIT order is similar to
    a Stop-Limit order, except that a LIT SELL order is placed above the current
    market price, and a Stop-Limit SELL order is placed below.

    Using a LIT order helps to ensure that, if the order does execute, the order
    will not execute at a price less favorable than the limit price.

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
    price : Price
        The order price (LIMIT).
    trigger_price : Price
        The order trigger price (STOP).
    trigger_type : TriggerType
        The order trigger type.
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        UNIX timestamp (nanoseconds) when the order will expire.
    post_only : bool, default False
        If the ``LIMIT`` order will only provide liquidity (once triggered).
    reduce_only : bool, default False
        If the ``LIMIT`` order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
    display_qty : Quantity, optional
        The quantity of the ``LIMIT`` order to display on the public book (iceberg).
    emulation_trigger : TriggerType, default ``NO_TRIGGER``
        The type of market price trigger to use for local order emulation.
        - ``NO_TRIGGER`` (default): Disables local emulation; orders are sent directly to the venue.
        - ``DEFAULT`` (the same as ``BID_ASK``): Enables local order emulation by triggering orders based on bid/ask prices.
        Additional trigger types are available. See the "Emulated Orders" section in the documentation for more details.
    trigger_instrument_id : InstrumentId, optional
        The emulation trigger instrument ID for the order (if ``None`` then will be the `instrument_id`).
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
        If `trigger_type` is ``NO_TRIGGER``.
    ValueError
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
    ValueError
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.
    ValueError
        If `display_qty` is negative (< 0) or greater than `quantity`.

    References
    ----------
    https://www.interactivebrokers.com/en/trading/orders/lit.php
    """

    price: Price
    trigger_price: Price
    trigger_type: TriggerType
    expire_time_ns: int
    display_qty: Quantity | None
    is_triggered: bool
    ts_triggered: int

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        trigger_price: Price,
        trigger_type: TriggerType,
        init_id: UUID4,
        ts_init: int,
        time_in_force: TimeInForce = ...,
        expire_time_ns: int = 0,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        contingency_type: ContingencyType = ...,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, Any] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: list[str] | None = None,
    ) -> None: ...
    @property
    def expire_time(self) -> datetime | None:
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
    def to_dict(self) -> dict[str, Any]:
        """
        Return a dictionary representation of this object.

        Returns
        -------
        dict[str, object]

        """
        ...
    @staticmethod
    def create(init) -> LimitIfTouchedOrder: ...
