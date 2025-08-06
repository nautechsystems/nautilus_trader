from decimal import Decimal
from typing import Any

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from stubs.core.fsm import FiniteStateMachine
from stubs.core.uuid import UUID4
from stubs.model.events.order import OrderEvent
from stubs.model.events.order import OrderInitialized
from stubs.model.identifiers import AccountId
from stubs.model.identifiers import ClientOrderId
from stubs.model.identifiers import ExecAlgorithmId
from stubs.model.identifiers import InstrumentId
from stubs.model.identifiers import PositionId
from stubs.model.identifiers import StrategyId
from stubs.model.identifiers import Symbol
from stubs.model.identifiers import TradeId
from stubs.model.identifiers import Venue
from stubs.model.identifiers import VenueOrderId
from stubs.model.objects import Currency
from stubs.model.objects import Money
from stubs.model.objects import Price
from stubs.model.objects import Quantity

STOP_ORDER_TYPES: set[OrderType]
LIMIT_ORDER_TYPES: set[OrderType]
LOCAL_ACTIVE_ORDER_STATUS: set[OrderStatus]

class Order:

    trader_id: StrategyId
    strategy_id: StrategyId
    instrument_id: InstrumentId
    client_order_id: ClientOrderId
    venue_order_id: VenueOrderId | None
    position_id: PositionId | None
    account_id: AccountId | None
    last_trade_id: TradeId | None
    side: OrderSide
    order_type: OrderType
    quantity: Quantity
    time_in_force: TimeInForce
    liquidity_side: Any
    is_post_only: bool
    is_reduce_only: bool
    is_quote_quantity: bool
    emulation_trigger: TriggerType | None
    trigger_instrument_id: InstrumentId | None
    contingency_type: Any
    order_list_id: Any | None
    linked_order_ids: list[ClientOrderId] | None
    parent_order_id: ClientOrderId | None
    exec_algorithm_id: ExecAlgorithmId | None
    exec_algorithm_params: dict[str, str] | None
    exec_spawn_id: ClientOrderId | None
    tags: list[str] | None
    filled_qty: Quantity
    leaves_qty: Quantity
    avg_px: float
    slippage: float
    init_id: UUID4
    ts_init: int
    ts_submitted: int
    ts_accepted: int
    ts_closed: int
    ts_last: int

    _events: list[OrderEvent]
    _venue_order_ids: list[VenueOrderId]
    _trade_ids: list[TradeId]
    _commissions: dict[Currency, Money]
    _fsm: FiniteStateMachine
    _previous_status: OrderStatus
    _triggered_price: Price | None

    def __init__(self, init: OrderInitialized) -> None: ...
    def __eq__(self, other: Order) -> bool: ...
    def __hash__(self) -> int: ...
    def __repr__(self) -> str: ...
    def status_string(self) -> str: ...
    def side_string(self) -> str: ...
    def type_string(self) -> str: ...
    def tif_string(self) -> str: ...
    def info(self) -> str: ...
    def to_dict(self) -> dict[str, Any]: ...
    def to_own_book_order(self) -> nautilus_pyo3.OwnBookOrder: ...
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
    def events(self) -> list[OrderEvent]: ...
    @property
    def venue_order_ids(self) -> list[VenueOrderId]: ...
    @property
    def trade_ids(self) -> list[TradeId]: ...
    @property
    def event_count(self) -> int: ...
    @property
    def has_price(self) -> bool: ...
    @property
    def has_trigger_price(self) -> bool: ...
    @property
    def has_activation_price(self) -> bool: ...
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
    @staticmethod
    def opposite_side(side: OrderSide) -> OrderSide: ...
    @staticmethod
    def closing_side(position_side: PositionSide) -> OrderSide: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def would_reduce_only(self, position_side: PositionSide, position_qty: Quantity) -> bool: ...
    def commissions(self) -> list[Money]: ...
    def apply(self, event: OrderEvent) -> None: ...
