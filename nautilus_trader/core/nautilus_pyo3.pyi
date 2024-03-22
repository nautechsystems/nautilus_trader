# ruff: noqa: UP007 PYI021 PYI044 PYI053
# fmt: off

import datetime as dt
from collections.abc import Awaitable
from collections.abc import Callable
from decimal import Decimal
from enum import Enum
from os import PathLike
from typing import Any, TypeAlias, Union

from nautilus_trader.core.data import Data

# Python Interface typing:
# We will eventually separate these into a .pyi file per module, for now this at least
# provides import resolution as well as docstrings.

###################################################################################################
# Core
###################################################################################################

class UUID4:
    def __init__(self, value: str) -> None: ...

def secs_to_nanos(secs: float) -> int:
    """
    Return round nanoseconds (ns) converted from the given seconds.

    Parameters
    ----------
    secs : float
        The seconds to convert.

    Returns
    -------
    int

    """

def secs_to_millis(secs: float) -> int:
    """
    Return round milliseconds (ms) converted from the given seconds.

    Parameters
    ----------
    secs : float
        The seconds to convert.

    Returns
    -------
    int

    """

def millis_to_nanos(millis: float) -> int:
    """
    Return round nanoseconds (ns) converted from the given milliseconds (ms).

    Parameters
    ----------
    millis : float
        The milliseconds to convert.

    Returns
    -------
    int

    """

def micros_to_nanos(micros: float) -> int:
    """
    Return round nanoseconds (ns) converted from the given microseconds (μs).

    Parameters
    ----------
    micros : float
        The microseconds to convert.

    Returns
    -------
    int

    """

def nanos_to_secs(nanos: int) -> float:
    """
    Return seconds converted from the given nanoseconds (ns).

    Parameters
    ----------
    nanos : int
        The nanoseconds to convert.

    Returns
    -------
    float

    """

def nanos_to_millis(nanos: int) -> int:
    """
    Return round milliseconds (ms) converted from the given nanoseconds (ns).

    Parameters
    ----------
    nanos : int
        The nanoseconds to convert.

    Returns
    -------
    int

    """

def nanos_to_micros(nanos: int) -> int:
    """
    Return round microseconds (μs) converted from the given nanoseconds (ns).

    Parameters
    ----------
    nanos : int
        The nanoseconds to convert.

    Returns
    -------
    int

    """

def last_weekday_nanos(year: int, month: int, day: int) -> int:
    """
    Return UNIX nanoseconds at midnight (UTC) of the last weekday (Mon-Fri).

    Parameters
    ----------
    year : int
        The year from the datum date.
    month : int
        The month from the datum date.
    day : int
        The day from the datum date.

    Returns
    -------
    int

    Raises
    ------
    ValueError
        If given an invalid date.

    """


def is_within_last_24_hours(timestamp_ns: int) -> bool:
    """
    Return whether the given UNIX nanoseconds timestamp is within the last 24 hours.

    Parameters
    ----------
    timestamp_ns : int
        The UNIX nanoseconds timestamp datum.

    Returns
    -------
    bool

    Raises
    ------
    ValueError
        If `timestamp` is invalid.

    """


def convert_to_snake_case(s: str) -> str:
    """
    Convert the given string from any common case (PascalCase, camelCase, kebab-case, etc.)
    to *lower* snake_case.

    This function uses the `heck` crate under the hood.

    Parameters
    ----------
    s : str
        The input string to convert.

    Returns
    -------
    str

    """

###################################################################################################
# Common
###################################################################################################

### Logging

class LogGuard:
    """
    Provides a `LogGuard` which serves as a token to signal the initialization
    of the logging system. It also ensures that the global logger is flushed
    of any buffered records when the instance is destroyed.

    """

def init_tracing() -> None:
    ...

def init_logging(
    trader_id: TraderId,
    instance_id: UUID4,
    level_stdout: LogLevel,
    level_file: LogLevel | None = None,
    component_levels: dict[str, str] | None = None,
    directory: str | None = None,
    file_name: str | None = None,
    file_format: str | None = None,
    is_colored: bool | None = None,
    is_bypassed: bool | None = None,
    print_config: bool | None = None,
) -> LogGuard: ...

def log_header(
    trader_id: TraderId,
    machine_id: str,
    instance_id: UUID4,
    component: str
) -> None: ...

def log_sysinfo(component: str) -> None: ...

###################################################################################################
# Model
###################################################################################################

### Accounting

class Position:
    def __init__(
        self,
        instrument: CurrencyPair | CryptoPerpetual | Equity | OptionsContract | SyntheticInstrument,
        fill: OrderFilled
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> Position: ...
    def to_dict(self) -> dict[str, str]: ...
    @property
    def id(self) -> PositionId: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    @property
    def opening_order_id(self) -> ClientOrderId: ...
    @property
    def closing_order_id(self) -> ClientOrderId | None: ...
    @property
    def quantity(self) -> Quantity: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def peak_qty(self) -> Quantity: ...
    @property
    def signed_qty(self) -> float: ...
    @property
    def entry(self) -> OrderSide: ...
    @property
    def side(self) -> PositionSide: ...
    @property
    def ts_opened(self) -> int: ...
    @property
    def duration_ns(self) -> int: ...
    @property
    def avg_px_open(self) -> Price: ...
    @property
    def event_count(self) -> int: ...
    @property
    def venue_order_ids(self) -> list[VenueOrderId]: ...
    @property
    def client_order_ids(self) -> list[ClientOrderId]: ...
    @property
    def trade_ids(self) -> list[TradeId]: ...
    @property
    def last_trade_id(self) -> TradeId | None: ...
    @property
    def events(self) -> list[OrderFilled]: ...
    @property
    def is_open(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_long(self) -> bool: ...
    @property
    def is_short(self) -> bool: ...
    @property
    def realized_return(self) -> float: ...
    @property
    def realized_pnl(self) -> Money | None : ...
    @property
    def ts_closed(self) -> int | None: ...
    @property
    def avg_px_close(self) -> Price | None: ...
    def unrealized_pnl(self, price: Price) -> Money: ...
    def total_pnl(self, price: Price) -> Money: ...
    def commissions(self) -> list[Money]: ...
    def apply(self, fill: OrderFilled) -> None: ...
    def is_opposite_side(self, side: OrderSide) -> bool: ...
    def calculate_pnl(self, avg_px_open: float, avg_px_close: float, quantity: Quantity) -> Money: ...
    def notional_value(self, price: Price) -> Money: ...


class MarginAccount:
    def __init__(
        self,
        event: AccountState,
        calculate_account_state: bool
    ) -> None: ...
    @property
    def id(self) -> AccountId: ...
    @property
    def default_leverage(self) -> float: ...
    def leverages(self) -> dict[InstrumentId, float]: ...
    def leverage(self,instrument_id: InstrumentId) -> float : ...
    def set_default_leverage(self, leverage: float) -> None: ...
    def set_leverage(self, instrument_id: InstrumentId, leverage: float) -> None: ...
    def is_unleveraged(self) -> bool: ...

    def update_initial_margin(self, instrument_id: InstrumentId, initial_margin: Money) -> None: ...

    def initial_margin(self, instrument_id: InstrumentId) -> Money: ...
    def initial_margins(self) -> dict[InstrumentId, Money]: ...

    def update_maintenance_margin(self, instrument_id: InstrumentId, maintenance_margin: Money) -> None: ...

    def maintenance_margin(self, instrument_id: InstrumentId) -> Money: ...
    def maintenance_margins(self) -> dict[InstrumentId, Money]: ...
    def calculate_initial_margin(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool | None = None
    ) -> Money: ...
    def calculate_maintenance_margin(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool | None = None
    ) -> Money: ...

class CashAccount:
    def __init__(
        self,
        event: AccountState,
        calculate_account_state: bool
    ) -> None: ...
    def to_dict(self) -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, str]) -> CashAccount: ...
    @property
    def id(self) -> AccountId: ...
    @property
    def base_currency(self) -> Currency | None: ...
    @property
    def last_event(self) -> AccountState | None: ...
    def events(self) -> list[AccountState]: ...
    @property
    def event_count(self) -> int: ...
    def balance_total(self, currency: Currency | None) -> Money | None : ...
    def balances_total(self) -> dict[Currency,Money]: ...
    def balance_free(self, currency: Currency | None) -> Money | None : ...

    def balances_free(self) -> dict[Currency,Money]: ...
    def balance_locked(self, currency: Currency | None) -> Money | None: ...

    def balances_locked(self) -> dict[Currency,Money]: ...
    def apply(self, event: AccountState) -> None: ...
    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool | None = None
    ) -> Money: ...
    def calculate_commission(
        self,
        instrument: Instrument,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: bool | None = None
    ) -> Money: ...
    def calculate_pnls(
        self,
        instrument: Instrument,
        fill: OrderFilled,
        position: Position | None = None
    ) -> list[Money]: ...

### Accounting transformers
def cash_account_from_account_events(events: list[dict],calculate_account_state) -> CashAccount: ...

def margin_account_from_account_events(events: list[dict],calculate_account_state) -> MarginAccount: ...


### Data types

def drop_cvec_pycapsule(capsule: object) -> None: ...

class BarSpecification:
    def __init__(
        self,
        step: int,
        aggregation: BarAggregation,
        price_type: PriceType,
    ) -> None: ...
    @property
    def step(self) -> int: ...
    @property
    def aggregation(self) -> BarAggregation: ...
    @property
    def price_type(self) -> PriceType: ...
    @property
    def timedelta(self) -> dt.timedelta: ...
    @classmethod
    def from_str(cls, value: str) -> BarSpecification: ...

class BarType:
    def __init__(
        self,
        instrument_id: InstrumentId,
        bar_spec: BarSpecification,
        aggregation_source: AggregationSource | None = None,
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def spec(self) -> BarSpecification: ...
    @property
    def aggregation_source(self) -> AggregationSource: ...
    @classmethod
    def from_str(cls, value: str) -> BarType: ...

class Bar:
    def __init__(
        self,
        bar_type: BarType,
        open: Price,
        high: Price,
        low: Price,
        close: Price,
        volume: Quantity,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def bar_type(self) -> BarType: ...
    @property
    def open(self) -> Price: ...
    @property
    def high(self) -> Price: ...
    @property
    def low(self) -> Price: ...
    @property
    def close(self) -> Price: ...
    @property
    def volume(self) -> Quantity: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...

class BookOrder:
    def __init__(
        self,
        side: OrderSide,
        price: Price,
        size: Quantity,
        order_id: int,
    ) -> None: ...
    @property
    def side(self) -> OrderSide: ...
    @property
    def price(self) -> Price: ...
    @property
    def size(self) -> Quantity: ...
    @property
    def order_id(self) -> int: ...

class OrderBookDelta:
    def __init__(
        self,
        instrument_id: InstrumentId,
        action: BookAction,
        order: BookOrder | None,
        flags: int,
        sequence: int,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def action(self) -> BookAction: ...
    @property
    def order(self) -> BookOrder: ...
    @property
    def flags(self) -> int: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...

class OrderBookDeltas:
    def __init__(
        self,
        instrument_id: InstrumentId,
        deltas: list[OrderBookDelta],
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def deltas(self) -> list[OrderBookDelta]: ...
    @property
    def flags(self) -> int: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

class OrderBookDepth10:
    def __init__(
        self,
        instrument_id: InstrumentId,
        bids: list[BookOrder],
        asks: list[BookOrder],
        bid_counts: list[int],
        ask_counts: list[int],
        flags: int,
        sequence: int,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def get_stub() -> OrderBookDepth10: ...

class QuoteTick:
    def __init__(
        self,
        instrument_id: InstrumentId,
        bid_price: Price,
        ask_price: Price,
        bid_size: Quantity,
        ask_size: Quantity,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...

class TradeTick:
    def __init__(
        self,
        instrument_id: InstrumentId,
        price: Price,
        size: Quantity,
        aggressor_side: AggressorSide,
        trade_id: TradeId,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> TradeTick: ...

### Enums

class AccountType(Enum):
    CASH = "CASH"
    MARGIN = "MARGIN"
    BETTING = "BETTING"

class AggregationSource(Enum):
    EXTERNAL = "EXTERNAL"
    INTERNAL = "INTERNAL"

class AggressorSide(Enum):
    BUYER = "BUYER"
    SELLER = "SELLER"

class AssetClass(Enum):
    FX = "FX"
    EQUITY = "EQUITY"
    COMMODITY = "COMMODITY"
    DEBT = "DEBT"
    INDEX = "INDEX"
    CRYPTOCURRENCY = "CRYPTOCURRENCY"
    ALTERNATIVE = "ALTERNATIVE"

class InstrumentClass(Enum):
    SPOT = "SPOT"
    SWAP = "SWAP"
    FUTURE = "FUTURE"
    FUTURE_SPREAD = "FUTURE_SPREAD"
    FORWARD = "FORWARD"
    CFD = "CFD"
    BOND = "BOND"
    OPTION = "OPTION"
    OPTION_SPREAD = "OPTION_SPEAD"
    WARRANT = "WARRANT"
    SPORTS_BETTING = "SPORTS_BETTING"

class BarAggregation(Enum):
    TICK = "TICK"
    TICK_IMBALANCE = "TICK_IMBALANCE"
    TICK_RUNS = "TICK_RUNS"
    VOLUME = "VOLUME"
    VOLUME_IMBALANCE = "VOLUME_IMBALANCE"
    VOLUME_RUNS = "VOLUME_RUNS"
    VALUE = "VALUE"
    VALUE_IMBALANCE = "VALUE_IMBALANCE"
    VALUE_RUNS = "VALUE_RUNS"
    MILLISECOND = "MILLISECOND"
    SECOND = "SECOND"
    MINUTE = "MINUTE"
    HOUR = "HOUR"
    DAY = "DAY"
    WEEK = "WEEK"
    MONTH = "MONTH"

class BookAction(Enum):
    ADD = "ADD"
    UPDATE = "UPDATE"
    DELETE = "DELETE"
    CLEAR = "CLEAR"

class BookType(Enum):
    L1_MBP = "L1_MBP"
    L2_MBP = "L2_MBP"
    L3_MBO = "L3_MBO"

class ContingencyType(Enum):
    OCO = "OCO"
    OTO = "OTO"
    OUO = "OUO"

class CurrencyType(Enum):
    CRYPTO = "CRYPTO"
    FIAT = "FIAT"
    COMMODITY_BACKED = "COMMODITY_BACKED"

class InstrumentCloseType(Enum):
    END_OF_SESSION = "END_OF_SESSION"
    CONTRACT_EXPIRED = "CONTRACT_EXPIRED"

class LiquiditySide(Enum):
    MAKER = "MAKER"
    TAKER = "TAKER"
    NO_LIQUIDITY_SIDE = "NO_LIQUIDITY_SIDE"

class MarketStatus(Enum):
    PRE_OPEN = "PRE_OPEN"
    OPEN = "OPEN"
    PAUSE = "PAUSE"
    HALT = "HALT"
    REOPEN = "REOPEN"
    PRE_CLOSE = "PRE_CLOSE"
    CLOSED = "CLOSED"

class HaltReason(Enum):
    NOT_HALTED = "NOT_HALTED"
    GENERAL = "GENERAL"
    VOLATILITY = "VOLATILITY"

class OmsType(Enum):
    UNSPECIFIED = "UNSPECIFIED"
    NETTING = "NETTING"
    HEDGING = "HEDGIN"

class OptionKind(Enum):
    CALL = "CALL"
    PUT = "PUT"

class OrderSide(Enum):
    NO_ORDER_SIDE = "NO_ORDER_SIDE"
    BUY = "BUY"
    SELL = "SELL"

class OrderStatus(Enum):
    INITIALIZED = "INITIALIZED"
    DENIED = "DENIED"
    EMULATED = "EMULATED"
    RELEASED = "RELEASED"
    SUBMITTED = "SUBMITTED"
    ACCEPTED = "ACCEPTED"
    REJECTED = "REJECTED"
    CANCELED = "CANCELED"
    EXPIRED = "EXPIRED"
    TRIGGERED = "TRIGGERED"
    PENDING_UPDATE = "PENDING_UPDATE"
    PENDING_CANCEL = "PENDING_CANCEL"
    PARTIALLY_FILLED = "PARTIALLY_FILLED"
    FILLED = "FILLED"

class OrderType(Enum):
    MARKET = "MARKET"
    LIMIT = "LIMIT"
    STOP_MARKET = "STOP_MARKET"
    STOP_LIMIT = "STOP_LIMIT"
    MARKET_TO_LIMIT = "MARKET_TO_LIMIT"
    MARKET_IF_TOUCHED = "MARKET_IF_TOUCHED"
    LIMIT_IF_TOUCHED = "LIMIT_IF_TOUCHED"
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"
    TRAILING_STOP_LIMIT = "TRAILING_STOP_LIMIT"

class PositionSide(Enum):
    FLAT = "FLAT"
    LONG = "LONG"
    SHORT = "SHORT"

class PriceType(Enum):
    BID = "BID"
    ASK = "ASK"
    MID = "MID"
    LAST = "LAST"

class TimeInForce(Enum):
    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    GTD = "GTD"
    DAY = "DAY"
    AT_THE_OPEN = "AT_THE_OPEN"
    AT_THE_CLOSE = "AT_THE_CLOSE"

class TradingState(Enum):
    ACTIVE = "ACTIVE"
    HALTED = "HALTED"
    REDUCING = "REDUCING"

class TrailingOffsetType(Enum):
    PRICE = "PRICE"
    BASIS_POINTS = "BASIS_POINTS"
    TICKS = "TICKS"
    PRICE_TIER = "PRICE_TIER"

class TriggerType(Enum):
    DEFAULT = "DEFAULT"
    BID_ASK = "BID_ASK"
    LAST_TRADE = "LAST_TRADE"
    DOUBLE_LAST = "DOUBLE_LAST"
    DOUBLE_BID_ASK = "DOUBLE_BID_ASK"
    LAST_OR_BID_ASK = "LAST_OR_BID_ASK"
    MID_POINT = "MID_POINT"
    MARK_PRICE = "MARK_PRICE"
    INDEX_PRICE = "INDEX_PRICE"

class MovingAverageType(Enum):
    SIMPLE = "SIMPLE"
    EXPONENTIAL = "EXPONENTIAL"
    DOUBLE_EXPONENTIAL = "DOUBLE_EXPONENTIAL"
    WILDER = "WILDER"
    HULL = "HULL"

class LogLevel(Enum):
    DEBUG = "DEBUG"
    INFO = "INFO"
    WARNING = "WARNING"
    ERROR = "ERROR"

class LogColor(Enum):
    DEFAULT = "DEFAULT"
    GREEN = "GREEN"
    BLUE = "BLUE"
    MAGENTA = "MAGENTA"
    CYAN = "CYAN"
    YELLOW = "YELLOW"
    RED = "RED"

### Identifiers

class AccountId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class ClientId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class ClientOrderId:
    def __init__(self, value: str) -> None: ...
    @property
    def value(self) -> str: ...

class ComponentId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class ExecAlgorithmId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class InstrumentId:
    def __init__(self, symbol: Symbol, venue: Venue) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> InstrumentId: ...
    @property
    def symbol(self) -> Symbol: ...
    @property
    def venue(self) -> Venue: ...
    def value(self) -> str: ...

class OrderListId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class PositionId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class StrategyId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class Symbol:
    def __init__(self, value: str) -> None: ...
    @property
    def value(self) -> str: ...

class TradeId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class TraderId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class Venue:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class VenueOrderId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

### Orders

class LimitOrder:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        price: Price,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        init_id: UUID4,
        ts_init: int,
        expire_time: int | None = None,
        display_qty: Quantity | None = None,
        emulation_trigger: TriggerType | None = None,
        trigger_instrument_id: InstrumentId | None = None,
        contingency_type: ContingencyType | None = None,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, str] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: str | None = None,
    ): ...
    def to_dict(self) -> dict[str, str]: ...
    @property
    def trader_id(self) -> TraderId: ...
    @property
    def strategy_id(self) -> StrategyId: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def client_order_id(self) -> ClientOrderId: ...
    @property
    def order_type(self) -> OrderType: ...
    @property
    def side(self) -> OrderSide: ...
    @property
    def quantity(self) -> Quantity: ...
    @property
    def price(self) -> Price: ...
    @property
    def expire_time(self) -> int | None: ...
    @property
    def status(self) -> OrderStatus: ...
    @property
    def time_in_force(self) -> TimeInForce: ...
    @property
    def is_post_only(self) -> bool: ...
    @property
    def is_reduce_only(self) -> bool: ...
    @property
    def is_quote_quantity(self) -> bool: ...
    @property
    def has_price(self) -> bool: ...
    @property
    def has_trigger_price(self) -> bool: ...
    @property
    def is_passive(self) -> bool: ...
    @property
    def is_aggressive(self) -> bool: ...
    @property
    def is_open(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_emulated(self) -> bool: ...
    @property
    def is_active_local(self) -> bool: ...
    @property
    def is_primary(self) -> bool: ...
    @property
    def is_spawned(self) -> bool: ...
    def from_dict(cls, values: dict[str, str]) -> LimitOrder: ...


class LimitIfTouchedOrder: ...

class MarketOrder:
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
        time_in_force: TimeInForce = ...,
        reduce_only: bool = False,
        quote_quantity: bool = False,
        contingency_type: ContingencyType | None = None,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, str] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: str | None = None,
    ) -> None: ...
    def to_dict(self) -> dict[str, str]: ...
    @staticmethod
    def opposite_side(side: OrderSide) -> OrderSide: ...
    @staticmethod
    def closing_side(side: PositionSide) -> OrderSide: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def would_reduce_only(self, side: PositionSide, position_qty: Quantity) -> bool: ...
    def commission(self, currency: Currency) -> Money | None: ...
    def commissions(self) -> dict[Currency, Money]: ...
    @property
    def trader_id(self) -> TraderId: ...
    @property
    def account_id(self) -> AccountId: ...
    @property
    def strategy_id(self) -> StrategyId: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def client_order_id(self) -> ClientOrderId: ...
    @property
    def venue_order_id(self) -> VenueOrderId | None: ...
    @property
    def position_id(self) -> PositionId | None: ...
    @property
    def last_trade_id(self) -> TradeId | None: ...
    @property
    def quantity(self) -> Quantity: ...
    @property
    def side(self) -> OrderSide: ...
    @property
    def order_type(self) -> OrderType: ...
    @property
    def price(self) -> Price | None: ...

class MarketToLimitOrder: ...
class StopLimitOrder: ...
class StopMarketOrder: ...
class TrailingStopLimitOrder: ...
class TrailingStopMarketOrder: ...

### Objects

class Currency:
    def __init__(
        self,
        code: str,
        precision: int,
        iso4217: int,
        name: str,
        currency_type: CurrencyType,
    ) -> None: ...
    @property
    def code(self) -> str: ...
    @property
    def precision(self) -> int: ...
    @property
    def iso4217(self) -> int: ...
    @property
    def name(self) -> str: ...
    @property
    def currency_type(self) -> CurrencyType: ...
    @staticmethod
    def is_fiat(code: str) -> bool: ...
    @staticmethod
    def is_crypto(code: str) -> bool: ...
    @staticmethod
    def is_commodity_backed(code: str) -> bool: ...
    @staticmethod
    def from_str(value: str, strict: bool = False) -> Currency: ...
    @staticmethod
    def register(currency: Currency, overwrite: bool = False) -> None: ...

class Money:
    def __init__(self, value: float, currency: Currency) -> None: ...
    @property
    def raw(self) -> int: ...
    @property
    def currency(self) -> Currency: ...
    @staticmethod
    def zero(currency: Currency) -> Money: ...
    @staticmethod
    def from_raw(raw: int, currency: Currency) -> Money: ...
    @staticmethod
    def from_str(value: str) -> Money: ...
    def is_zero(self) -> bool: ...
    def as_decimal(self) -> Decimal: ...
    def as_double(self) -> float: ...
    def to_formatted_str(self) -> str: ...

class Price:
    def __init__(self, value: float, precision: int) -> None: ...
    @property
    def raw(self) -> int: ...
    @property
    def precision(self) -> int: ...
    @staticmethod
    def from_raw(raw: int, precision: int) -> Price: ...
    @staticmethod
    def zero(precision: int = 0) -> Price: ...
    @staticmethod
    def from_int(value: int) -> Price: ...
    @staticmethod
    def from_str(value: str) -> Price: ...
    def is_zero(self) -> bool: ...
    def is_positive(self) -> bool: ...
    def as_double(self) -> float: ...
    def as_decimal(self) -> Decimal: ...
    def to_formatted_str(self) -> str: ...

class Quantity:
    def __init__(self, value: float, precision: int) -> None: ...
    @property
    def raw(self) -> int: ...
    @property
    def precision(self) -> int: ...
    @staticmethod
    def from_raw(raw: int, precision: int) -> Quantity: ...
    @staticmethod
    def zero(precision: int = 0) -> Quantity: ...
    @staticmethod
    def from_int(value: int) -> Quantity: ...
    @staticmethod
    def from_str(value: str) -> Quantity: ...
    def is_zero(self) -> bool: ...
    def is_positive(self) -> bool: ...
    def as_decimal(self) -> Decimal: ...
    def as_double(self) -> float: ...
    def to_formatted_str(self) -> str: ...

class AccountBalance:
    def __init__(self, total: Money, locked: Money, free: Money): ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> AccountBalance: ...
    def to_dict(self) -> dict[str, str]: ...

class MarginBalance:
    def __init__(self, initial: Money, maintenance: Money, instrument_id: InstrumentId): ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> MarginBalance: ...
    def to_dict(self) -> dict[str, str]: ...

class AccountState:
    def __init__(
        self,
        account_id: AccountId,
        account_type: AccountType,
        base_currency: Currency,
        balances: list[AccountBalance],
        margins: list[MarginBalance],
        is_reported: bool,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> AccountState: ...
    def to_dict(self) -> dict[str, str]: ...

### Instruments

class CryptoFuture:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        underlying: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        activation_ns: int,
        expiration_ns: int,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        margin_init: Decimal,
        margin_maint: Decimal,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> CryptoFuture: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class CryptoPerpetual:
    def __init__(
        self,
        id: InstrumentId,
        symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        margin_init: Decimal,
        margin_maint: Decimal,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> CryptoPerpetual: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class CurrencyPair:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        base_currency: Currency,
        quote_currency: Currency,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        margin_init: Decimal,
        margin_maint: Decimal,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> CurrencyPair: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class Equity:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        ts_event: int,
        ts_init: int,
        isin: str | None = None,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> Equity: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class FuturesContract:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: str,
        activation_ns: int,
        expiration_ns: int,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        ts_event: int,
        ts_init: int,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        exchange: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> CryptoFuture: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class FuturesSpread:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: str,
        strategy_type: str,
        activation_ns: int,
        expiration_ns: int,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        exchange: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> FuturesSpread: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class OptionsContract:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: str,
        option_kind: OptionKind,
        activation_ns: int,
        expiration_ns: int,
        strike_price: Price,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        exchange: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None : ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OptionsContract: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class OptionsSpread:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        underlying: str,
        strategy_type: str,
        activation_ns: int,
        expiration_ns: int,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        lot_size: Quantity,
        ts_event: int,
        ts_init: int,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
        margin_init: Decimal | None = None,
        margin_maint: Decimal | None = None,
        exchange: str | None = None,
        info: dict[str, Any] | None = None,
    ) -> None : ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OptionsContract: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class SyntheticInstrument:
    @property
    def id(self) -> InstrumentId: ...
    @property
    def base_currency(self) -> Currency: ...
    @property
    def quote_currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

Instrument: TypeAlias = Union[
    CryptoFuture,
    CryptoPerpetual,
    CurrencyPair,
    Equity,
    FuturesContract,
    OptionsContract,
    SyntheticInstrument,
]


### Events

class OrderDenied:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderDenied: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderTriggered:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderRejected: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderRejected:
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
        reconciliation: bool,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderRejected: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderFilled:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        venue_order_id: VenueOrderId,
        account_id: AccountId,
        trade_id: TradeId,
        order_side: OrderSide,
        order_type: OrderType,
        last_qty: Quantity,
        last_px: Price,
        currency: Currency,
        liquidity_side: LiquiditySide,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        position_id: PositionId | None = None,
        commission: Money | None = None,
    )-> None: ...
    @property
    def is_buy(self) -> bool: ...
    @property
    def is_sell(self) -> bool: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderFilled: ...
    def to_dict(self) -> dict[str, str]: ...
    @property
    def order_side(self) -> OrderSide: ...
    @property
    def client_order_id(self) -> ClientOrderId: ...

class OrderInitialized:
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
        reconciliation: bool,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        price: Price | None = None,
        trigger_price: Price | None = None,
        trigger_type: TriggerType | None = None,
        limit_offset: Price | None = None,
        trailing_offset: Price | None = None,
        trailing_offset_type: TrailingOffsetType | None = None,
        expire_time: int | None = None,
        display_quantity: Quantity | None = None,
        emulation_trigger: TriggerType | None = None,
        trigger_instrument_id: InstrumentId | None = None,
        contingency_type: ContingencyType | None = None,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, str] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: str | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderInitialized: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderSubmitted:
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
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderSubmitted: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderEmulated:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None : ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderEmulated: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderReleased:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        released_price: Price,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderReleased: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderUpdated:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        quantity: Quantity,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
        price: Price | None = None,
        trigger_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderUpdated: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderPendingUpdate:
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
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderPendingUpdate: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderPendingCancel:
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
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderPendingCancel: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderModifyRejected:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderModifyRejected: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderAccepted:
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
        reconciliation: bool,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderAccepted: ...
    def to_dict(self) -> dict[str, str]: ...


class OrderCancelRejected:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        reason: str,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
    )-> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderCancelRejected: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderCanceled:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
    ) -> None: ...

    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderCanceled: ...
    def to_dict(self) -> dict[str, str]: ...

class OrderExpired:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        event_id: UUID4,
        ts_event: int,
        ts_init: int,
        reconciliation: bool,
        venue_order_id: VenueOrderId | None = None,
        account_id: AccountId | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderExpired: ...
    def to_dict(self) -> dict[str, str]: ...

class Level:
    @property
    def price(self) -> Price: ...
    def len(self) -> int: ...
    def is_empty(self) -> bool: ...
    def size(self) -> float: ...
    def size_raw(self) -> int: ...
    def exposure(self) -> float: ...
    def exposure_raw(self) -> int: ...
    def first(self) -> BookOrder | None: ...
    def get_orders(self) -> list[BookOrder]: ...

class OrderBookMbo:
    def __init__(self, instrument_id: InstrumentId) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def book_type(self) -> BookType: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @property
    def ts_last(self) -> int: ...
    @property
    def count(self) -> int: ...
    def reset(self) -> None: ...
    def update(self, order: BookOrder, ts_event: int, sequence: int = 0) -> None: ...
    def delete(self, order: BookOrder, ts_event: int, sequence: int = 0) -> None: ...
    def clear(self, ts_event: int, sequence: int = 0) -> None: ...
    def clear_bids(self, ts_event: int, sequence: int = 0) -> None: ...
    def clear_asks(self, ts_event: int, sequence: int = 0) -> None: ...
    def apply_delta(self, delta: OrderBookDelta) -> None: ...
    def apply_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def apply_depth(self, depth: OrderBookDepth10) -> None: ...
    def check_integrity(self) -> None: ...
    def bids(self) -> list[Level]: ...
    def asks(self) -> list[Level]: ...
    def best_bid_price(self) -> Price | None: ...
    def best_ask_price(self) -> Price | None: ...
    def best_bid_size(self) -> Quantity | None: ...
    def best_ask_size(self) -> Quantity | None: ...
    def spread(self) -> float | None: ...
    def midpoint(self) -> float | None: ...
    def get_avg_px_for_quantity(self, qty: Quantity, order_side: OrderSide) -> float: ...
    def get_quantity_for_price(self, price: Price, order_side: OrderSide) -> float: ...
    def simulate_fills(self, order: BookOrder) -> list[tuple[Price, Quantity]]: ...
    def pprint(self, num_levels: int) -> str: ...

class OrderBookMbp:
    def __init__(
        self,
        instrument_id: InstrumentId,
        top_only: bool = False,
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def book_type(self) -> BookType: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @property
    def ts_last(self) -> int: ...
    @property
    def count(self) -> int: ...
    def reset(self) -> None: ...
    def update(self, order: BookOrder, ts_event: int, sequence: int = 0) -> None: ...
    def update_quote_tick(self, quote: QuoteTick) -> None: ...
    def update_trade_tick(self, trade: TradeTick) -> None: ...
    def delete(self, order: BookOrder, ts_event: int, sequence: int = 0) -> None: ...
    def clear(self, ts_event: int, sequence: int = 0) -> None: ...
    def clear_bids(self, ts_event: int, sequence: int = 0) -> None: ...
    def clear_asks(self, ts_event: int, sequence: int = 0) -> None: ...
    def apply_delta(self, delta: OrderBookDelta) -> None: ...
    def apply_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def apply_depth(self, depth: OrderBookDepth10) -> None: ...
    def check_integrity(self) -> None: ...
    def bids(self) -> list[Level]: ...
    def asks(self) -> list[Level]: ...
    def best_bid_price(self) -> Price | None: ...
    def best_ask_price(self) -> Price | None: ...
    def best_bid_size(self) -> Quantity | None: ...
    def best_ask_size(self) -> Quantity | None: ...
    def spread(self) -> float | None: ...
    def midpoint(self) -> float | None: ...
    def get_avg_px_for_quantity(self, qty: Quantity, order_side: OrderSide) -> float: ...
    def get_quantity_for_price(self, price: Price, order_side: OrderSide) -> float: ...
    def simulate_fills(self, order: BookOrder) -> list[tuple[Price, Quantity]]: ...
    def pprint(self, num_levels: int) -> str: ...

###################################################################################################
# Infrastructure
###################################################################################################

class RedisCacheDatabase:
    def __init__(
        self,
        trader_id: TraderId,
        config: dict[str, Any],
    ) -> None: ...

###################################################################################################
# Network
###################################################################################################

class HttpClient:
    def __init__(
        self,
        header_keys: list[str] = [],
        keyed_quotas: list[tuple[str, Quota]] = [],
        default_quota: Quota | None = None,
    ) -> None: ...
    async def request(
        self,
        method: HttpMethod,
        url: str,
        headers: dict[str, str] | None = None,
        body: bytes | None = None,
        keys: list[str] | None = None,
    ) -> HttpResponse: ...

class HttpMethod(Enum):
    GET = "GET"
    POST = "POST"
    PUT = "PUT"
    DELETE = "DELETE"
    PATCH = "PATCH"

class HttpResponse:
    @property
    def status(self) -> int: ...
    @property
    def body(self) -> bytes: ...
    @property
    def headers(self) -> dict[str, str]: ...

class Quota:
    @classmethod
    def rate_per_second(cls, max_burst: int) -> Quota: ...
    @classmethod
    def rate_per_minute(cls, max_burst: int) -> Quota: ...
    @classmethod
    def rate_per_hour(cls, max_burst: int) -> Quota: ...

class WebSocketConfig:
    def __init__(
        self,
        url: str,
        handler: Callable[..., Any],
        headers: list[tuple[str, str]],
        heartbeat: int | None = None,
        heartbeat_msg: str | None = None,
        ping_handler: Callable[..., Any] | None = None,
    ) -> None: ...

class WebSocketClient:
    @classmethod
    def connect(
        cls,
        config: WebSocketConfig,
        post_connection: Callable[..., None] | None = None,
        post_reconnection: Callable[..., None] | None = None,
        post_disconnection: Callable[..., None] | None = None,
    ) -> Awaitable[WebSocketClient]: ...
    def disconnect(self) -> Any: ...
    @property
    def is_alive(self) -> bool: ...
    def send(self, data: bytes) -> Awaitable[None]: ...
    def send_text(self, data: str) -> Awaitable[None]: ...
    def send_pong(self, data: bytes) -> Awaitable[None]: ...

class SocketClient:
    @classmethod
    def connect(
        cls,
        config: SocketConfig,
        post_connection: Callable[..., None] | None = None,
        post_reconnection: Callable[..., None] | None = None,
        post_disconnection: Callable[..., None] | None = None,
    ) -> Awaitable[SocketClient]: ...
    def disconnect(self) -> None: ...
    @property
    def is_alive(self) -> bool: ...
    def send(self, data: bytes) -> Awaitable[None]: ...

class SocketConfig:
    def __init__(
        self,
        url: str,
        ssl: bool,
        suffix: bytes,
        handler: Callable[..., Any],
        heartbeat: tuple[int, list[int]] | None = None,
    ) -> None: ...

###################################################################################################
# Persistence
###################################################################################################

class NautilusDataType(Enum):
    OrderBookDelta = 1
    OrderBookDepth10 = 2
    QuoteTick = 3
    TradeTick = 4
    Bar = 5

class DataBackendSession:
    def __init__(self, chunk_size: int = 5000) -> None: ...
    def add_file(
        self,
        data_type: NautilusDataType,
        table_name: str,
        file_path: str,
        sql_query: str | None = None,
    ) -> None: ...
    def to_query_result(self) -> DataQueryResult: ...

class QueryResult:
    def next(self) -> Data | None: ...

class DataQueryResult:
    def __init__(self, result: QueryResult, size: int) -> None: ...
    def drop_chunk(self) -> None: ...
    def __iter__(self) -> DataQueryResult: ...
    def __next__(self) -> Any | None: ...

class DataTransformer:
    @staticmethod
    def get_schema_map(data_cls: type) -> dict[str, str]: ...
    @staticmethod
    def pyobjects_to_record_batch_bytes(data: list[Data]) -> bytes: ...
    @staticmethod
    def pyo3_order_book_deltas_to_record_batch_bytes(data: list[OrderBookDelta]) -> bytes: ...
    @staticmethod
    def pyo3_order_book_depth10_to_record_batch_bytes(data: list[OrderBookDepth10]) -> bytes: ...
    @staticmethod
    def pyo3_quote_ticks_to_record_batch_bytes(data: list[QuoteTick]) -> bytes: ...
    @staticmethod
    def pyo3_trade_ticks_to_record_batch_bytes(data: list[TradeTick]) -> bytes: ...
    @staticmethod
    def pyo3_bars_to_record_batch_bytes(data: list[Bar]) -> bytes: ...

class OrderBookDeltaDataWrangler:
    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None: ...
    @property
    def instrument_id(self) -> str: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    def process_record_batch_bytes(self, data: bytes) -> list[OrderBookDelta]: ...

class QuoteTickDataWrangler:
    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None: ...
    @property
    def instrument_id(self) -> str: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    def process_record_batch_bytes(self, data: bytes) -> list[QuoteTick]: ...

class TradeTickDataWrangler:
    def __init__(
        self,
        instrument_id: str,
        price_precision: int,
        size_precision: int,
    ) -> None: ...
    @property
    def instrument_id(self) -> str: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    def process_record_batch_bytes(self, data: bytes) -> list[TradeTick]: ...

class BarDataWrangler:
    def __init__(
        self,
        bar_type: str,
        price_precision: int,
        size_precision: int,
    ) -> None: ...
    @property
    def bar_type(self) -> str: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    def process_record_batch_bytes(self, data: bytes) -> list[Bar]: ...


###################################################################################################
# Indicators
###################################################################################################

class SimpleMovingAverage:
    def __init__(
        self,
        period: int,
        price_type: PriceType | None = None,
    )-> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def reset(self) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...

class ExponentialMovingAverage:
    def __init__(
        self,
        period: int,
        price_type: PriceType | None = None,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def alpha(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class DoubleExponentialMovingAverage:
    def __init__(
        self,
        period: int,
        price_type: PriceType | None = None
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class HullMovingAverage:
    def __init__(
        self,
        period: int,
        price_type: PriceType | None = None
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class WilderMovingAverage:
    def __init__(
        self,
        period: int,
        price_type: PriceType | None = None,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def alpha(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class VariableIndexDynamicAverage:
    def __init__(
        self,
        period: int,
        cmo_ma_type: int,
        price_type: PriceType | None = None,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def alpha(self) -> float: ...
    @property
    def cmo(self) -> ChandeMomentumOscillator: ...
    @property
    def cmo_pct(self) -> float: ...
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class ChandeMomentumOscillator:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class AroonOscillator:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def aroon_up(self) -> float: ...
    @property
    def aroon_down(self) -> float: ...
    def update_raw(self, high: float, low: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class AverageTrueRange:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        use_previous: bool = True,
        value_floor: float = 0.0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

# Book

class BookImbalanceRatio:
    def __init__(self) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def count(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def handle_book_mbo(self, book: OrderBookMbo) -> None:...
    def handle_book_mbp(self, book: OrderBookMbp) -> None:...
    def update(self, best_bid: Quantity | None, best_ask: Quantity) -> None: ...
    def reset(self) -> None: ...

###################################################################################################
# Adapters
###################################################################################################

# Databento

class DatabentoStatisticType(Enum):
    OPENING_PRICE = "OPENING_PRICE"
    INDICATIVE_OPENING_PRICE = "INDICATIVE_OPENING_PRICE"
    SETTLEMENT_PRICE = "SETTLEMENT_PRICE"
    TRADING_SESSION_LOW_PRICE = "TRADING_SESSION_LOW_PRICE"
    TRADING_SESSION_HIGH_PRICE = "TRADING_SESSION_HIGH_PRICE"
    CLEARED_VOLUME = "CLEARED_VOLUME"
    LOWEST_OFFER = "LOWEST_OFFER"
    HIGHEST_BID = "HIGHEST_BID"
    OPEN_INTEREST = "OPEN_INTEREST"
    FIXING_PRICE = "FIXING_PRICE"
    CLOSE_PRICE = "CLOSE_PRICE"
    NET_CHANGE = "NET_CHANGE"
    VWAP = "VWAP"

class DatabentoStatisticUpdateAction(Enum):
    ADDED = "ADDED"
    DELETED = "DELETED"

class DatabentoPublisher:
    @property
    def publisher_id(self) -> int: ...
    @property
    def dataset(self) -> str: ...
    @property
    def venue(self) -> str: ...
    @property
    def description(self) -> str: ...

class DatabentoImbalance:
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def ref_price(self) -> Price: ...
    @property
    def cont_book_clr_price(self) -> Price: ...
    @property
    def auct_interest_clr_price(self) -> Price: ...
    @property
    def paired_qty(self) -> Quantity: ...
    @property
    def total_imbalance_qty(self) -> Quantity: ...
    @property
    def side(self) -> OrderSide: ...
    @property
    def significant_imbalance(self) -> str: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

class DatabentoStatistics:
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def stat_type(self) -> DatabentoStatisticType: ...
    @property
    def update_action(self) -> DatabentoStatisticUpdateAction: ...
    @property
    def price(self) -> Price | None: ...
    @property
    def quantity(self) -> Quantity | None: ...
    @property
    def channel_id(self) -> int: ...
    @property
    def stat_flags(self) -> int: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_ref(self) -> int: ...
    @property
    def ts_in_delta(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_recv(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

class DatabentoDataLoader:
    def __init__(
        self,
        path: PathLike[str] | str,
    ) -> None: ...
    def load_publishers(self, path: PathLike[str] | str) -> None: ...
    def get_publishers(self) -> dict[int, DatabentoPublisher]: ...
    def get_dataset_for_venue(self, venue: Venue) -> str: ...
    def schema_for_file(self, path: str) -> str: ...
    def load_instruments(self, path: str) -> list[Instrument]: ...
    def load_order_book_deltas(self, path: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> list[OrderBookDelta]: ...
    def load_order_book_deltas_as_pycapsule(self, path: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> object: ...
    def load_order_book_depth10(self, path: str, instrument_id: InstrumentId | None) -> list[OrderBookDepth10]: ...
    def load_order_book_depth10_as_pycapsule(self, path: str, instrument_id: InstrumentId | None) -> object: ...
    def load_quotes(self, path: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> list[QuoteTick]: ...
    def load_quotes_as_pycapsule(self, path: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> object: ...
    def load_trades(self, path: str, instrument_id: InstrumentId | None) -> list[TradeTick]: ...
    def load_trades_as_pycapsule(self, path: str, instrument_id: InstrumentId | None) -> object: ...
    def load_bars(self, path: str, instrument_id: InstrumentId | None) -> list[Bar]: ...
    def load_bars_as_pycapsule(self, path: str, instrument_id: InstrumentId | None) -> object: ...
    def load_imbalance(self, path: str, instrument_id: InstrumentId | None) -> list[DatabentoImbalance]: ...
    def load_statistics(self, path: str, instrument_id: InstrumentId | None) -> list[DatabentoStatistics]: ...

class DatabentoHistoricalClient:
    def __init__(
        self,
        key: str,
        publishers_path: str,
    ) -> None: ...
    @property
    def key(self) -> str: ...
    async def get_dataset_range(self, dataset: str) -> dict[str, str]: ...
    async def get_range_instruments(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[Instrument]: ...
    async def get_range_quotes(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[QuoteTick]: ...
    async def get_range_trades(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[TradeTick]: ...
    async def get_range_bars(
        self,
        dataset: str,
        symbols: list[str],
        aggregation: BarAggregation,
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[Bar]: ...
    async def get_range_imbalance(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[DatabentoImbalance]: ...
    async def get_range_statistics(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[DatabentoStatistics]: ...

class DatabentoLiveClient:
    def __init__(
        self,
        key: str,
        dataset: str,
        publishers_path: str,
    ) -> None: ...
    @property
    def key(self) -> str: ...
    @property
    def dataset(self) -> str: ...
    @property
    def is_running(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    def subscribe(
        self,
        schema: str,
        symbols: list[str],
        stype_in: str | None = None,
        start: int | None = None,
    ) -> dict[str, str]: ...
    def start(
        self,
        callback: Callable,
        callback_pyo3: Callable,
    ) -> Awaitable[None]: ...
    def close(self) -> None: ...
