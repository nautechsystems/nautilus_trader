from collections.abc import Callable
from typing import Any, ClassVar


class OrderSide:
    BUY: ClassVar[OrderSide]
    SELL: ClassVar[OrderSide]


class OrderType:
    MARKET: ClassVar[OrderType]
    LIMIT: ClassVar[OrderType]
    STOP_MARKET: ClassVar[OrderType]
    STOP_LIMIT: ClassVar[OrderType]


class TimeInForce:
    DAY: ClassVar[TimeInForce]
    GTC: ClassVar[TimeInForce]
    IOC: ClassVar[TimeInForce]
    FOK: ClassVar[TimeInForce]


class OrderStatus:
    PENDING: ClassVar[OrderStatus]
    OPEN: ClassVar[OrderStatus]
    PARTIAL: ClassVar[OrderStatus]
    COMPLETE: ClassVar[OrderStatus]
    CANCELLED: ClassVar[OrderStatus]
    REJECTED: ClassVar[OrderStatus]
    EXPIRED: ClassVar[OrderStatus]


class ConnectionState:
    DISCONNECTED: ClassVar[ConnectionState]
    CONNECTING: ClassVar[ConnectionState]
    CONNECTED: ClassVar[ConnectionState]
    RECONNECTING: ClassVar[ConnectionState]
    ERROR: ClassVar[ConnectionState]


class QuoteTick:
    symbol: str
    exchange: str
    bid_price: float
    ask_price: float
    bid_size: float
    ask_size: float
    ts_event: int
    ts_init: int


class TradeTick:
    symbol: str
    exchange: str
    price: float
    size: float
    aggressor_side: str
    trade_id: str
    ts_event: int
    ts_init: int


class TimeBar:
    symbol: str
    exchange: str
    open_price: float
    high_price: float
    low_price: float
    close_price: float
    volume: int
    period: str
    bar_kind: str
    bar_period: int
    marker: int | None
    ts_event: int
    ts_init: int


class MarketDataEvent:
    def is_quote(self) -> bool: ...
    def is_trade(self) -> bool: ...
    def is_bar(self) -> bool: ...
    def is_error(self) -> bool: ...
    def as_quote(self) -> QuoteTick: ...
    def as_trade(self) -> TradeTick: ...
    def as_bar(self) -> TimeBar: ...
    def as_error(self) -> str: ...


class OrderSubmitted:
    is_snapshot: bool
    client_order_id: str
    venue_order_id: str | None
    account_id: str
    symbol: str | None
    exchange: str | None
    side: str | None
    order_type: str | None
    time_in_force: str | None
    quantity: float | None
    filled_qty: float | None
    leaves_qty: float | None
    price: float | None
    trigger_price: float | None
    avg_price: float | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class OrderAccepted:
    is_snapshot: bool
    client_order_id: str
    venue_order_id: str
    account_id: str
    symbol: str | None
    exchange: str | None
    side: str | None
    order_type: str | None
    time_in_force: str | None
    quantity: float | None
    filled_qty: float | None
    leaves_qty: float | None
    price: float | None
    trigger_price: float | None
    avg_price: float | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class OrderRejected:
    is_snapshot: bool
    client_order_id: str
    reason: str
    symbol: str | None
    exchange: str | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class OrderFilled:
    is_snapshot: bool
    client_order_id: str
    venue_order_id: str
    fill_price: float
    fill_qty: float
    leaves_qty: float
    commission: float
    symbol: str | None
    exchange: str | None
    side: str | None
    trade_id: str | None
    currency: str | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class OrderCancelled:
    is_snapshot: bool
    client_order_id: str
    venue_order_id: str
    symbol: str | None
    exchange: str | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class OrderModified:
    is_snapshot: bool
    client_order_id: str
    venue_order_id: str
    new_price: float | None
    new_qty: float | None
    symbol: str | None
    exchange: str | None
    original_basket_id: str | None
    linked_basket_ids: list[str]
    bracket_type: str | None
    ts_event: int


class ExecutionEvent:
    def is_error(self) -> bool: ...
    def is_submitted(self) -> bool: ...
    def is_accepted(self) -> bool: ...
    def is_rejected(self) -> bool: ...
    def is_filled(self) -> bool: ...
    def is_cancelled(self) -> bool: ...
    def is_modified(self) -> bool: ...
    def as_error(self) -> str: ...
    def as_submitted(self) -> OrderSubmitted: ...
    def as_accepted(self) -> OrderAccepted: ...
    def as_rejected(self) -> OrderRejected: ...
    def as_filled(self) -> OrderFilled: ...
    def as_cancelled(self) -> OrderCancelled: ...
    def as_modified(self) -> OrderModified: ...


class AccountEvent:
    is_snapshot: bool
    account_id: str
    currency: str
    total: float
    available: float
    locked: float
    unrealized_pnl: float
    realized_pnl: float


class PositionEvent:
    is_snapshot: bool
    account_id: str
    symbol: str
    exchange: str
    quantity: float
    avg_price: float
    unrealized_pnl: float
    realized_pnl: float
    ts_event: int


class RithmicGateway:
    @staticmethod
    def from_env(profile: str | None = ...) -> RithmicGateway: ...
    def __init__(
        self,
        *,
        environment: Any,
        username: str,
        password: str,
        system_name: str,
        app_name: str,
        app_version: str,
        fcm_id: str,
        ib_id: str,
        account_id: str,
        server: str | None = ...,
        alt_server: str | None = ...,
        enable_ticker: bool,
        enable_order: bool,
        enable_pnl: bool,
        enable_history: bool,
    ) -> None: ...
    async def connect(self) -> None: ...
    async def disconnect(self) -> None: ...
    def is_connected(self) -> bool: ...
    async def list_accounts(self) -> list[str]: ...
    async def request_pnl_snapshot(self) -> None: ...
    def start_pnl_loop(self, callback: Callable[[AccountEvent | PositionEvent], None]) -> None: ...
    def stop_pnl_loop(self) -> None: ...
    def connection_state(self) -> str: ...
    def account_id(self) -> str | None: ...


class RithmicInstrument:
    symbol: str
    exchange: str
    product_code: str
    description: str
    tick_size: float
    point_value: float
    currency: str
    contract_size: float
    price_precision: int
    size_precision: int
    expiration_ts: int | None
    is_tradeable: bool


class RithmicInstrumentProvider:
    def __init__(self, gateway: RithmicGateway) -> None: ...
    async def load_all_async(self) -> None: ...
    async def load_exchange_async(self, exchange: str) -> list[RithmicInstrument]: ...
    async def load_instrument_async(self, symbol: str, exchange: str) -> RithmicInstrument: ...
    async def load_front_month_async(self, product: str, exchange: str) -> RithmicInstrument: ...
    def instruments(self) -> list[RithmicInstrument]: ...


class RithmicDataClient:
    def __init__(self, gateway: RithmicGateway) -> None: ...
    def set_data_callback(self, callback: Callable[[MarketDataEvent], None]) -> None: ...
    def clear_data_callback(self) -> None: ...
    async def start_event_loop(self) -> None: ...
    def stop_event_loop(self) -> None: ...
    def unsubscribe_all(self) -> None: ...
    async def subscribe_quotes(self, symbol: str, exchange: str) -> None: ...
    async def subscribe_trades(self, symbol: str, exchange: str) -> None: ...
    async def unsubscribe(self, symbol: str, exchange: str) -> None: ...
    async def request_bars(self, *args: Any, **kwargs: Any) -> list[Any]: ...


class RithmicExecutionClient:
    def __init__(self, gateway: RithmicGateway, account_id: str) -> None: ...
    def set_execution_callback(self, callback: Callable[[ExecutionEvent], None]) -> None: ...
    def clear_execution_callback(self) -> None: ...
    async def start_event_loop(self) -> None: ...
    def stop_event_loop(self) -> None: ...
    async def submit_order(
        self,
        *,
        symbol: str,
        exchange: str,
        side: OrderSide,
        order_type: OrderType,
        quantity: int,
        client_order_id: str,
        price: float | None = ...,
        stop_price: float | None = ...,
        time_in_force: TimeInForce | None = ...,
        trailing_stop_ticks: int | None = ...,
    ) -> None: ...
    async def submit_bracket_order(
        self,
        *,
        symbol: str,
        exchange: str,
        side: OrderSide,
        order_type: OrderType,
        quantity: int,
        client_order_id: str,
        profit_ticks: int,
        stop_ticks: int,
        price: float | None = ...,
        time_in_force: TimeInForce | None = ...,
    ) -> None: ...
    async def submit_oco_order(
        self,
        *,
        leg1_symbol: str,
        leg1_exchange: str,
        leg1_side: OrderSide,
        leg1_order_type: OrderType,
        leg1_quantity: int,
        leg1_client_order_id: str,
        leg1_price: float | None = ...,
        leg1_stop_price: float | None = ...,
        leg1_time_in_force: TimeInForce | None = ...,
        leg2_symbol: str,
        leg2_exchange: str,
        leg2_side: OrderSide,
        leg2_order_type: OrderType,
        leg2_quantity: int,
        leg2_client_order_id: str,
        leg2_price: float | None = ...,
        leg2_stop_price: float | None = ...,
        leg2_time_in_force: TimeInForce | None = ...,
    ) -> None: ...
    async def modify_order(
        self,
        *,
        venue_order_id: str,
        symbol: str,
        exchange: str,
        new_qty: int | None = ...,
        new_price: float | None = ...,
        order_type: OrderType | None = ...,
    ) -> None: ...
    async def cancel_order(self, venue_order_id: str) -> None: ...
    async def cancel_all_orders(self) -> None: ...
    async def batch_cancel_orders(self, ids: list[str]) -> None: ...
    async def show_brackets(self) -> list[dict[str, str | None]]: ...
    async def show_bracket_stops(self) -> list[dict[str, str | None]]: ...
