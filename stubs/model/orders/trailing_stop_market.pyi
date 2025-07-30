from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.core.nautilus_pyo3 import UUID4
from nautilus_trader.core.nautilus_pyo3 import ClientOrderId
from nautilus_trader.core.nautilus_pyo3 import ContingencyType
from nautilus_trader.core.nautilus_pyo3 import ExecAlgorithmId
from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import Order
from nautilus_trader.core.nautilus_pyo3 import OrderInitialized
from nautilus_trader.core.nautilus_pyo3 import OrderListId
from nautilus_trader.core.nautilus_pyo3 import OrderSide
from nautilus_trader.core.nautilus_pyo3 import Price
from nautilus_trader.core.nautilus_pyo3 import Quantity
from nautilus_trader.core.nautilus_pyo3 import StrategyId
from nautilus_trader.core.nautilus_pyo3 import TimeInForce
from nautilus_trader.core.nautilus_pyo3 import TraderId
from nautilus_trader.core.nautilus_pyo3 import TrailingOffsetType
from nautilus_trader.core.nautilus_pyo3 import TriggerType

class TrailingStopMarketOrder(Order):
    """
    Represents a `Trailing-Stop-Market` conditional order.

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
    trigger_price : Price or ``None``
        The order trigger price (STOP). If ``None`` then will typically default
        to the delta of market price and `trailing_offset`.
    trigger_type : TriggerType
        The order trigger type.
    trailing_offset : Decimal
        The trailing offset for the trigger price (STOP).
    trailing_offset_type : TrailingOffsetType
        The order trailing offset type.
    init_id : UUID4
        The order initialization event ID.
    ts_init : uint64_t
        UNIX timestamp (nanoseconds) when the object was initialized.
    activation_price : Price, optional
        The price for the order to become active. If ``None`` then the order will be activated right after the order is accepted.
    time_in_force : TimeInForce {``GTC``, ``IOC``, ``FOK``, ``GTD``, ``DAY``}, default ``GTC``
        The order time in force.
    expire_time_ns : uint64_t, default 0 (no expiry)
        UNIX timestamp (nanoseconds) when the order will expire.
    reduce_only : bool, default False
        If the order carries the 'reduce-only' execution instruction.
    quote_quantity : bool, default False
        If the order quantity is denominated in the quote currency.
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
        If `trailing_offset_type` is ``NO_TRAILING_OFFSET``.
    ValueError
        If `time_in_force` is ``AT_THE_OPEN`` or ``AT_THE_CLOSE``.
    ValueError
        If `time_in_force` is ``GTD`` and `expire_time_ns` <= UNIX epoch.
    """

    activation_price: Price | None
    trigger_price: Price | None
    trigger_type: TriggerType
    trailing_offset: Decimal
    trailing_offset_type: TrailingOffsetType
    expire_time_ns: int
    is_activated: bool

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price | None,
        trigger_type: TriggerType,
        trailing_offset: Decimal,
        trailing_offset_type: TrailingOffsetType,
        init_id: UUID4,
        ts_init: int,
        activation_price: Price | None = None,
        time_in_force: TimeInForce = ...,
        expire_time_ns: int = 0,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        emulation_trigger: TriggerType = ...,
        trigger_instrument_id: InstrumentId | None = None,
        contingency_type: ContingencyType = ...,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict | None = None,
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
    def create(init: OrderInitialized) -> TrailingStopMarketOrder: ...
