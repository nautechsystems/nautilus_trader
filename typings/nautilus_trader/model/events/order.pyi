from typing import Any, Dict, List, Optional

from nautilus_trader.core.message import Event
from nautilus_trader.core.model import (
    ContingencyType,
    LiquiditySide,
    OrderSide,
    OrderType,
    TimeInForce,
    TriggerType,
)
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    PositionId,
    StrategyId,
    TradeId,
    TraderId,
    VenueOrderId,
)
from nautilus_trader.model.objects import Currency, Money, Price, Quantity

class OrderEvent(Event):
    @property
    def trader_id(self) -> TraderId: ...
    @property
    def strategy_id(self) -> StrategyId: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def client_order_id(self) -> ClientOrderId: ...
    @property
    def venue_order_id(self) -> Optional[VenueOrderId]: ...
    @property
    def account_id(self) -> Optional[AccountId]: ...
    @property
    def reconciliation(self) -> bool: ...
    @property
    def id(self) -> UUID4: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    def set_client_order_id(self, client_order_id: ClientOrderId) -> None: ...

class OrderInitialized(OrderEvent):
    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    time_in_force: TimeInForce
    post_only: bool
    reduce_only: bool
    quote_quantity: bool
    options: Dict[str, Any]
    emulation_trigger: TriggerType
    trigger_instrument_id: Optional[InstrumentId]
    contingency_type: ContingencyType
    order_list_id: Optional[OrderListId]
    linked_order_ids: Optional[List[ClientOrderId]]
    parent_order_id: Optional[ClientOrderId]
    exec_algorithm_id: Optional[ExecAlgorithmId]
    exec_algorithm_params: Optional[Dict[str, Any]]
    exec_spawn_id: Optional[ClientOrderId]
    tags: Optional[List[str]]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        order_type: OrderType,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        options: Dict[str, Any],
        emulation_trigger: TriggerType,
        trigger_instrument_id: Optional[InstrumentId],
        contingency_type: ContingencyType,
        order_list_id: Optional[OrderListId],
        linked_order_ids: Optional[List[ClientOrderId]],
        parent_order_id: Optional[ClientOrderId],
        exec_algorithm_id: Optional[ExecAlgorithmId],
        exec_algorithm_params: Optional[Dict[str, Any]],
        exec_spawn_id: Optional[ClientOrderId],
        tags: Optional[List[str]],
        event_id: UUID4,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderInitialized: ...
    @staticmethod
    def to_dict(obj: OrderInitialized) -> Dict[str, Any]: ...

class OrderDenied(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    @property
    def reason(self) -> str: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderDenied: ...
    @staticmethod
    def to_dict(obj: OrderDenied) -> Dict[str, Any]: ...

class OrderEmulated(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderEmulated: ...
    @staticmethod
    def to_dict(obj: OrderEmulated) -> Dict[str, Any]: ...

class OrderReleased(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        released_price: Price,
        event_id: UUID4,
        ts_init: int,
    ) -> None: ...
    @property
    def released_price(self) -> Price: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderReleased: ...
    @staticmethod
    def to_dict(obj: OrderReleased) -> Dict[str, Any]: ...

class OrderSubmitted(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderSubmitted: ...
    @staticmethod
    def to_dict(obj: OrderSubmitted) -> Dict[str, Any]: ...

class OrderAccepted(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderAccepted: ...
    @staticmethod
    def to_dict(obj: OrderAccepted) -> Dict[str, Any]: ...

class OrderRejected(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        account_id: AccountId,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @property
    def reason(self) -> str: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderRejected: ...
    @staticmethod
    def to_dict(obj: OrderRejected) -> Dict[str, Any]: ...

class OrderCanceled(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderCanceled: ...
    @staticmethod
    def to_dict(obj: OrderCanceled) -> Dict[str, Any]: ...

class OrderExpired(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderExpired: ...
    @staticmethod
    def to_dict(obj: OrderExpired) -> Dict[str, Any]: ...

class OrderTriggered(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderTriggered: ...
    @staticmethod
    def to_dict(obj: OrderTriggered) -> Dict[str, Any]: ...

class OrderPendingUpdate(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderPendingUpdate: ...
    @staticmethod
    def to_dict(obj: OrderPendingUpdate) -> Dict[str, Any]: ...

class OrderPendingCancel(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderPendingCancel: ...
    @staticmethod
    def to_dict(obj: OrderPendingCancel) -> Dict[str, Any]: ...

class OrderModifyRejected(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @property
    def reason(self) -> str: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderModifyRejected: ...
    @staticmethod
    def to_dict(obj: OrderModifyRejected) -> Dict[str, Any]: ...

class OrderCancelRejected(OrderEvent):
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @property
    def reason(self) -> str: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderCancelRejected: ...
    @staticmethod
    def to_dict(obj: OrderCancelRejected) -> Dict[str, Any]: ...

class OrderUpdated(OrderEvent):
    quantity: Quantity
    price: Optional[Price]
    trigger_price: Optional[Price]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: Optional[VenueOrderId],
        account_id: Optional[AccountId],
        quantity: Quantity,
        price: Optional[Price],
        trigger_price: Optional[Price],
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
    ) -> None: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderUpdated: ...
    @staticmethod
    def to_dict(obj: OrderUpdated) -> Dict[str, Any]: ...

class OrderFilled(OrderEvent):
    trade_id: TradeId
    position_id: Optional[PositionId]
    order_side: OrderSide
    order_type: OrderType
    last_qty: Quantity
    last_px: Price
    currency: Currency
    commission: Money
    liquidity_side: LiquiditySide
    info: Dict[str, Any]

    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        position_id: Optional[PositionId],
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        commission: Money,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool = False,
        info: Optional[Dict[str, Any]] = None,
    ) -> None: ...
    @property
    def is_buy(self) -> bool: ...
    @property
    def is_sell(self) -> bool: ...
    @staticmethod
    def from_dict(values: Dict[str, Any]) -> OrderFilled: ...
    @staticmethod
    def to_dict(obj: OrderFilled) -> Dict[str, Any]: ...

__all__ = [
    "Event",
    "OrderEvent",
    "OrderInitialized",
    "OrderDenied",
    "OrderEmulated",
    "OrderReleased",
    "OrderSubmitted",
    "OrderRejected",
    "OrderAccepted",
    "OrderCanceled",
    "OrderExpired",
    "OrderTriggered",
    "OrderPendingUpdate",
    "OrderPendingCancel",
    "OrderModifyRejected",
    "OrderCancelRejected",
    "OrderUpdated",
    "OrderFilled",
]
