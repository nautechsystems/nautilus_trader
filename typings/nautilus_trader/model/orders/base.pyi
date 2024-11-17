from decimal import Decimal
from typing import Dict, List, Optional, Set

from nautilus_trader.core.model import (
    ContingencyType,
    LiquiditySide,
    OrderSide,
    OrderStatus,
    OrderType,
    PositionSide,
    TimeInForce,
    TriggerType,
)
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.events.order import (
    OrderEvent,
    OrderInitialized,
)
from nautilus_trader.model.identifiers import (
    AccountId,
    ClientOrderId,
    ExecAlgorithmId,
    InstrumentId,
    OrderListId,
    PositionId,
    StrategyId,
    Symbol,
    TradeId,
    TraderId,
    Venue,
    VenueOrderId,
)
from nautilus_trader.model.objects import Money, Quantity

STOP_ORDER_TYPES: Set[OrderType]
LIMIT_ORDER_TYPES: Set[OrderType]
LOCAL_ACTIVE_ORDER_STATUS: Set[OrderStatus]

class Order:
    trader_id: TraderId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    client_order_id: ClientOrderId
    venue_order_id: Optional[VenueOrderId]
    position_id: Optional[PositionId]
    account_id: Optional[AccountId]
    last_trade_id: Optional[TradeId]
    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    time_in_force: TimeInForce
    liquidity_side: LiquiditySide
    is_post_only: bool
    is_reduce_only: bool
    is_quote_quantity: bool
    emulation_trigger: TriggerType
    trigger_instrument_id: Optional[InstrumentId]
    contingency_type: ContingencyType
    order_list_id: Optional[OrderListId]
    linked_order_ids: Optional[List[ClientOrderId]]
    parent_order_id: Optional[ClientOrderId]
    exec_algorithm_id: Optional[ExecAlgorithmId]
    exec_algorithm_params: Optional[Dict[str, object]]
    exec_spawn_id: Optional[ClientOrderId]
    tags: List[str]
    filled_qty: Quantity
    leaves_qty: Quantity
    avg_px: float
    slippage: float
    init_id: UUID4
    ts_init: int
    ts_last: int

    def __init__(self, init: OrderInitialized) -> None: ...
    def __eq__(self, other: Order) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    @property
    def status(self) -> OrderStatus: ...
    @property
    def init_event(self) -> OrderInitialized: ...
    @property
    def last_event(self) -> OrderEvent: ...
    @property
    def events(self) -> List[OrderEvent]: ...
    @property
    def venue_order_ids(self) -> List[VenueOrderId]: ...
    @property
    def trade_ids(self) -> List[TradeId]: ...
    @property
    def event_count(self) -> int: ...
    @property
    def has_price(self) -> bool: ...
    @property
    def has_trigger_price(self) -> bool: ...
    @property
    def is_buy(self) -> bool: ...
    @property
    def is_sell(self) -> bool: ...
    @property
    def is_passive(self) -> bool: ...
    @property
    def is_aggressive(self) -> bool: ...
    @property
    def is_emulated(self) -> bool: ...
    @property
    def is_active_local(self) -> bool: ...
    @property
    def is_primary(self) -> bool: ...
    @property
    def is_spawned(self) -> bool: ...
    @property
    def is_contingency(self) -> bool: ...
    @property
    def is_parent_order(self) -> bool: ...
    @property
    def is_child_order(self) -> bool: ...
    @property
    def is_inflight(self) -> bool: ...
    @property
    def is_open(self) -> bool: ...
    @property
    def is_canceled(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_pending_update(self) -> bool: ...
    @property
    def is_pending_cancel(self) -> bool: ...
    def status_string(self) -> str: ...
    def side_string(self) -> str: ...
    def type_string(self) -> str: ...
    def tif_string(self) -> str: ...
    def info(self) -> str: ...
    def to_dict(self) -> Dict[str, object]: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def would_reduce_only(
        self, position_side: PositionSide, position_qty: Quantity
    ) -> bool: ...
    def commissions(self) -> List[Money]: ...
    def apply(self, event: OrderEvent) -> None: ...
    @staticmethod
    def opposite_side(side: OrderSide) -> OrderSide: ...
    @staticmethod
    def closing_side(position_side: PositionSide) -> OrderSide: ...
