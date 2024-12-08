from datetime import datetime
from typing import Any, Dict, List, Optional

from nautilus_trader.core.model import (
    ContingencyType,
    OrderSide,
    TimeInForce,
    TriggerType,
)
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.events.order import (
    OrderInitialized,
)
from nautilus_trader.model.identifiers import (
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.base import Order

class StopLimitOrder(Order):
    price: Price
    trigger_price: Price
    trigger_type: TriggerType
    expire_time_ns: int
    display_qty: Optional[Quantity]
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
        time_in_force: TimeInForce = TimeInForce.GTC,
        expire_time_ns: int = 0,
        post_only: bool = False,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        display_qty: Optional[Quantity] = None,
        emulation_trigger: TriggerType = TriggerType.NO_TRIGGER,
        trigger_instrument_id: Optional[InstrumentId] = None,
        contingency_type: ContingencyType = ContingencyType.NO_CONTINGENCY,
        order_list_id: Optional[OrderListId] = None,
        linked_order_ids: Optional[List[ClientOrderId]] = None,
        parent_order_id: Optional[ClientOrderId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict[str, Any]] = None,
        exec_spawn_id: Optional[ClientOrderId] = None,
        tags: Optional[List[str]] = None,
    ) -> None: ...
    @property
    def expire_time(self) -> Optional[datetime]: ...
    def info(self) -> str: ...
    def to_dict(self) -> Dict[str, Any]: ...
    @staticmethod
    def create(init: OrderInitialized) -> StopLimitOrder: ...
    @staticmethod
    def from_pyo3(pyo3_order: Any) -> StopLimitOrder: ...
