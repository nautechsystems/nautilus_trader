from datetime import datetime
from typing import Any

from nautilus_trader.common.enums import LogLevel
from nautilus_trader.model.enums import OrderSide
from stubs.core.message import Command
from stubs.core.uuid import UUID4
from stubs.model.identifiers import ClientId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import TraderId
from stubs.model.identifiers import VenueOrderId
from stubs.model.objects import Price
from stubs.model.objects import Quantity
from stubs.model.orders.base import Order
from stubs.model.orders.list import OrderList

class ExecutionReportCommand(Command):
    instrument_id: InstrumentId | None
    start: datetime | None
    end: datetime | None
    params: dict[str, Any]
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GenerateOrderStatusReport(ExecutionReportCommand):

    client_order_id: ClientOrderId | None
    venue_order_id: VenueOrderId | None

    def __init__(
        self,
        instrument_id: InstrumentId | None,
        client_order_id: ClientOrderId | None,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GenerateOrderStatusReports(ExecutionReportCommand):
    open_only: bool
    log_receipt_level: LogLevel
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        open_only: bool,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
        log_receipt_level: LogLevel = ...,
    ) -> None: ...

class GenerateFillReports(ExecutionReportCommand):
    venue_order_id: VenueOrderId | None
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        venue_order_id: VenueOrderId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class GeneratePositionStatusReports(ExecutionReportCommand):
    def __init__(
        self,
        instrument_id: InstrumentId | None,
        start: datetime | None,
        end: datetime | None,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class TradingCommand(Command):
    client_id: ClientId | None
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    params: dict[str, Any]
    def __init__(
        self,
        client_id: ClientId | None,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        command_id: UUID4,
        ts_init: int,
        params: dict[str, Any] | None = None,
    ) -> None: ...

class SubmitOrder(TradingCommand):
    order: Order
    exec_algorithm_id: ExecAlgorithmId | None
    position_id: PositionId | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order: Order,
        command_id: UUID4,
        ts_init: int,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> SubmitOrder: ...
    @staticmethod
    def to_dict(obj: SubmitOrder) -> dict[str, Any]: ...

class SubmitOrderList(TradingCommand):

    order_list: OrderList
    exec_algorithm_id: ExecAlgorithmId | None
    position_id: PositionId | None
    has_emulated_order: bool

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        order_list: OrderList,
        command_id: UUID4,
        ts_init: int,
        position_id: PositionId | None = None,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> SubmitOrderList: ...
    @staticmethod
    def to_dict(obj: SubmitOrderList) -> dict[str, Any]: ...

class ModifyOrder(TradingCommand):
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    quantity: Quantity | None
    price: Price | None
    trigger_price: Price | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        quantity: Quantity | None,
        price: Price | None,
        trigger_price: Price | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> ModifyOrder: ...
    @staticmethod
    def to_dict(obj: ModifyOrder) -> dict[str, Any]: ...

class CancelOrder(TradingCommand):
    
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> CancelOrder: ...
    @staticmethod
    def to_dict(obj: CancelOrder) -> dict[str, Any]: ...

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
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> CancelAllOrders: ...
    @staticmethod
    def to_dict(obj: CancelAllOrders) -> dict[str, Any]: ...

class BatchCancelOrders(TradingCommand):
    cancels: list[CancelOrder]
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        cancels: list,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> BatchCancelOrders: ...
    @staticmethod
    def to_dict(obj: BatchCancelOrders) -> dict[str, Any]: ...

class QueryOrder(TradingCommand):
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId | None,
        command_id: UUID4,
        ts_init: int,
        client_id: ClientId | None = None,
        params: dict[str, Any] | None = None,
    ) -> None: ...
    def __str__(self) -> str: ...
    def __repr__(self) -> str: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> QueryOrder: ...
    @staticmethod
    def to_dict(obj: QueryOrder) -> dict[str, Any]: ...