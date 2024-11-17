from typing import Any, Dict, List, Optional

from nautilus_trader.core.model import ContingencyType, OrderSide, TimeInForce
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.model.identifiers import (
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    StrategyId,
    TraderId,
)
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orders.base import Order

class MarketOrder(Order):
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
        time_in_force: TimeInForce = TimeInForce.GTC,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        contingency_type: ContingencyType = ContingencyType.NO_CONTINGENCY,
        order_list_id: Optional[OrderListId] = None,
        linked_order_ids: Optional[List[ClientOrderId]] = None,
        parent_order_id: Optional[ClientOrderId] = None,
        exec_algorithm_id: Optional[ExecAlgorithmId] = None,
        exec_algorithm_params: Optional[Dict[str, Any]] = None,
        exec_spawn_id: Optional[ClientOrderId] = None,
        tags: Optional[List[str]] = None,
    ) -> None: ...
    def info(self) -> str: ...
    @staticmethod
    def from_pyo3(pyo3_order: Any) -> MarketOrder: ...
    def to_dict(self) -> Dict[str, Any]: ...
    @staticmethod
    def create(init: OrderInitialized) -> MarketOrder: ...
    @staticmethod
    def transform_py(order: Order, ts_init: int) -> MarketOrder: ...
