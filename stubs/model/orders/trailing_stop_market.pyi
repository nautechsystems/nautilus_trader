from datetime import datetime
from decimal import Decimal
from typing import Any

from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
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

class TrailingStopMarketOrder(Order):

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
    def expire_time(self) -> datetime | None: ...
    def info(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...
    @staticmethod
    def create(init: OrderInitialized) -> TrailingStopMarketOrder: ...
