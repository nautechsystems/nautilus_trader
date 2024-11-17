from typing import Any, Dict, List, Optional

from nautilus_trader.core.message import Command
from nautilus_trader.core.model import OrderSide
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import (
    ClientId,
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    PositionId,
    StrategyId,
    TraderId,
    VenueOrderId,
)
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.model.orders.base import Order
from nautilus_trader.model.orders.list import OrderList

class TradingCommand(Command):
    client_id: Optional[ClientId]
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId

    def __init__(
        self,
        client_id: Optional[ClientId],
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        command_id: UUID4,
        ts_init: int,
    ) -> None: ...

class SubmitOrder(TradingCommand):
    order: Order
    exec_algorithm_id: Optional[ExecAlgorithmId]
    position_id: Optional[PositionId]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order: Order,
        command_id: UUID4,
        ts_init: int,
        position_id: Optional[PositionId] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> SubmitOrder: ...
    @staticmethod
    def to_dict(obj: SubmitOrder) -> Dict[str, Any]: ...

class SubmitOrderList(TradingCommand):
    order_list: OrderList
    exec_algorithm_id: Optional[ExecAlgorithmId]
    position_id: Optional[PositionId]
    has_emulated_order: bool

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order_list: OrderList,
        command_id: UUID4,
        ts_init: int,
        position_id: Optional[PositionId] = None,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> SubmitOrderList: ...
    @staticmethod
    def to_dict(obj: SubmitOrderList) -> Dict[str, Any]: ...

class ModifyOrder(TradingCommand):
    client_order_id: ClientOrderId
    venue_order_id: Optional[VenueOrderId]
    quantity: Optional[Quantity]
    price: Optional[Price]
    trigger_price: Optional[Price]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        quantity: Optional[Quantity],
        price: Optional[Price],
        trigger_price: Optional[Price],
        command_id: UUID4,
        ts_init: int,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> ModifyOrder: ...
    @staticmethod
    def to_dict(obj: ModifyOrder) -> Dict[str, Any]: ...

class CancelOrder(TradingCommand):
    client_order_id: ClientOrderId
    venue_order_id: Optional[VenueOrderId]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        command_id: UUID4,
        ts_init: int,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> CancelOrder: ...
    @staticmethod
    def to_dict(obj: CancelOrder) -> Dict[str, Any]: ...

class CancelAllOrders(TradingCommand):
    order_side: OrderSide

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        order_side: OrderSide,
        command_id: UUID4,
        ts_init: int,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> CancelAllOrders: ...
    @staticmethod
    def to_dict(obj: CancelAllOrders) -> Dict[str, Any]: ...

class BatchCancelOrders(TradingCommand):
    cancels: List[CancelOrder]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        cancels: List[CancelOrder],
        command_id: UUID4,
        ts_init: int,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> BatchCancelOrders: ...
    @staticmethod
    def to_dict(obj: BatchCancelOrders) -> Dict[str, Any]: ...

class QueryOrder(TradingCommand):
    client_order_id: ClientOrderId
    venue_order_id: Optional[VenueOrderId]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        command_id: UUID4,
        ts_init: int,
        client_id: Optional[ClientId] = None,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> QueryOrder: ...
    @staticmethod
    def to_dict(obj: QueryOrder) -> Dict[str, Any]: ...
