# ruff: noqa: UP007 PYI021 PYI044 PYI053
# fmt: off

import datetime as dt
from collections.abc import Awaitable
from collections.abc import Callable
from decimal import Decimal
from enum import Enum
from os import PathLike
from typing import Any, Final, TypeAlias, Union

import numpy as np

from nautilus_trader.core.data import Data

# Python Interface typing:
# We will eventually separate these into a .pyi file per module, for now this at least
# provides import resolution as well as docstrings.

###################################################################################################
# Core
###################################################################################################

NAUTILUS_VERSION: Final[str]
USER_AGENT: Final[str]

MILLISECONDS_IN_SECOND: Final[int]
NANOSECONDS_IN_SECOND: Final[int]
NANOSECONDS_IN_MILLISECOND: Final[int]
NANOSECONDS_IN_MICROSECOND: Final[int]

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

def convert_to_snake_case(input: str) -> str:
    """
    Convert the given string from any common case (PascalCase, camelCase, kebab-case, etc.)
    to *lower* snake_case.

    This function uses the `heck` Rust crate under the hood.

    Parameters
    ----------
    input : str
        The input string to convert.

    Returns
    -------
    str

    """

###################################################################################################
# Common
###################################################################################################

# Logging

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
    component: str,
) -> None: ...

def log_sysinfo(component: str) -> None: ...

# Message passing

class PythonMessageHandler:
    def __init__(
        self,
        id: str,
        handler: object
    ) -> None: ...

class MessageBus:
    def send(self, endpoint: str, message: object) -> None: ...
    def publish(self, topic: str, message: object) -> None: ...
    def register(self, endpoint: str, handler: PythonMessageHandler) -> None: ...
    def subscribe(self, topic: str, handler: PythonMessageHandler, priority: int) -> None: ...
    def is_subscribed(self, topic: str, handler: PythonMessageHandler) -> bool: ...
    def unsubscribe(self, topic: str, handler: PythonMessageHandler) -> None: ...
    def is_registered(self, endpoint: str) -> bool: ...
    def deregister(self, endpoint: str) -> None: ...

class Signal:
    def __init__(
        self,
        name: str,
        value: str,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def value(self) -> str: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

class CustomData:
    def __init__(
        self,
        data_type: DataType,
        value: bytes,
        ts_event: int,
        ts_init: int,
    ) -> None: ...
    @property
    def data_type(self) -> DataType: ...
    @property
    def value(self) -> str: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

###################################################################################################
# Cryptography
###################################################################################################

def hmac_signature(secret: str, data: str) -> str: ...
def rsa_signature(private_key_pem: str, data: str) -> str: ...
def ed25519_signature(private_key: bytes, data: str) -> str: ...

###################################################################################################
# Model
###################################################################################################

class DataType:
    def __init__(self, type_name: str, metadata: dict[str, str] | None = None) -> None: ...
    @property
    def type_name(self) -> str: ...
    @property
    def metadata(self) -> dict[str, str] | None: ...
    @property
    def topic(self) -> str: ...

# Accounting

class Position:
    def __init__(
        self,
        instrument: CurrencyPair | CryptoPerpetual | Equity | OptionsContract | SyntheticInstrument,
        fill: OrderFilled,
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
    def realized_pnl(self) -> Money | None: ...
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
        calculate_account_state: bool,
    ) -> None: ...
    @property
    def id(self) -> AccountId: ...
    @property
    def default_leverage(self) -> float: ...
    def leverages(self) -> dict[InstrumentId, float]: ...
    def leverage(self, instrument_id: InstrumentId) -> float: ...
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
        use_quote_for_inverse: bool | None = None,
    ) -> Money: ...
    def calculate_maintenance_margin(
        self,
        instrument: Instrument,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool | None = None,
    ) -> Money: ...

class CashAccount:
    def __init__(
        self,
        event: AccountState,
        calculate_account_state: bool,
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
    def balance_total(self, currency: Currency | None) -> Money | None: ...
    def balances_total(self) -> dict[Currency, Money]: ...
    def balance_free(self, currency: Currency | None) -> Money | None: ...
    def balances_free(self) -> dict[Currency, Money]: ...
    def balance_locked(self, currency: Currency | None) -> Money | None: ...
    def balances_locked(self) -> dict[Currency, Money]: ...
    def apply(self, event: AccountState) -> None: ...
    def calculate_balance_locked(
        self,
        instrument: Instrument,
        side: OrderSide,
        quantity: Quantity,
        price: Price,
        use_quote_for_inverse: bool | None = None,
    ) -> Money: ...
    def calculate_commission(
        self,
        instrument: Instrument,
        last_qty: Quantity,
        last_px: Price,
        liquidity_side: LiquiditySide,
        use_quote_for_inverse: bool | None = None,
    ) -> Money: ...
    def calculate_pnls(
        self,
        instrument: Instrument,
        fill: OrderFilled,
        position: Position | None = None,
    ) -> list[Money]: ...

Account: TypeAlias = Union[
    CashAccount,
    MarginAccount
]

# Accounting transformers

def cash_account_from_account_events(
    events: list[dict],
    calculate_account_state: bool,
) -> CashAccount: ...

def margin_account_from_account_events(
    events: list[dict],
    calculate_account_state: bool,
) -> MarginAccount: ...

# Data types

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
    @staticmethod
    def fully_qualified_name() -> str: ...

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
    @staticmethod
    def fully_qualified_name() -> str: ...
    @staticmethod
    def from_str(value: str) -> BarType: ...

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
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> Bar: ...
    @staticmethod
    def from_json(data: bytes) -> Bar: ...
    @staticmethod
    def from_msgpack(data: bytes) -> Bar: ...

    def as_pycapsule(self) -> object: ...
    def as_dict(self) -> dict[str, Any]: ...
    def as_json(self) -> bytes: ...
    def as_msgpack(self) -> bytes: ...

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
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderBookDelta: ...
    @staticmethod
    def from_json(data: bytes) -> OrderBookDelta: ...
    @staticmethod
    def from_msgpack(data: bytes) -> OrderBookDelta: ...

    def as_pycapsule(self) -> object: ...
    def as_dict(self) -> dict[str, Any]: ...
    def as_json(self) -> bytes: ...
    def as_msgpack(self) -> bytes: ...

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

    @staticmethod
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderBookDeltas: ...
    @staticmethod
    def from_json(data: bytes) -> OrderBookDeltas: ...
    @staticmethod
    def from_msgpack(data: bytes) -> OrderBookDeltas: ...

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
    def instrument_id(self) -> InstrumentId: ...
    @property
    def bids(self) -> list[BookOrder]: ...
    @property
    def asks(self) -> list[BookOrder]: ...
    @property
    def bid_counts(self) -> list[int]: ...
    @property
    def ask_counts(self) -> list[int]: ...
    @property
    def flags(self) -> int: ...
    @property
    def sequence(self) -> int: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

    @staticmethod
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def get_stub() -> OrderBookDepth10: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> OrderBookDepth10: ...
    @staticmethod
    def from_json(data: bytes) -> OrderBookDepth10: ...
    @staticmethod
    def from_msgpack(data: bytes) -> OrderBookDepth10: ...

    def as_pycapsule(self) -> object: ...
    def as_dict(self) -> dict[str, Any]: ...
    def as_json(self) -> bytes: ...
    def as_msgpack(self) -> bytes: ...

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
    def instrument_id(self) -> InstrumentId: ...
    @property
    def bid_price(self) -> Price: ...
    @property
    def ask_price(self) -> Price: ...
    @property
    def bid_size(self) -> Quantity: ...
    @property
    def ask_size(self) -> Quantity: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

    @staticmethod
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> QuoteTick: ...
    @staticmethod
    def from_json(data: bytes) -> QuoteTick: ...
    @staticmethod
    def from_msgpack(data: bytes) -> QuoteTick: ...

    def extract_price(self) -> Price: ...
    def extract_size(self) -> Quantity: ...
    def as_pycapsule(self) -> object: ...
    def as_dict(self) -> dict[str, Any]: ...
    def as_json(self) -> bytes: ...
    def as_msgpack(self) -> bytes: ...

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
    def instrument_id(self) -> InstrumentId: ...
    @property
    def price(self) -> Price: ...
    @property
    def size(self) -> Quantity: ...
    @property
    def aggressor_side(self) -> AggressorSide: ...
    @property
    def trade_id(self) -> TradeId: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...

    @staticmethod
    def fully_qualified_name() -> str: ...
    @staticmethod
    def get_metadata() -> dict[str, str]: ...
    @staticmethod
    def get_fields() -> dict[str, str]: ...
    @staticmethod
    def from_dict(values: dict[str, Any]) -> TradeTick: ...
    @staticmethod
    def from_json(data: bytes) -> TradeTick: ...
    @staticmethod
    def from_msgpack(data: bytes) -> TradeTick: ...

    def as_pycapsule(self) -> object: ...
    def as_dict(self) -> dict[str, Any]: ...
    def as_json(self) -> bytes: ...
    def as_msgpack(self) -> bytes: ...

class InstrumentStatus:
    def __init__(
        self,
        instrument_id: InstrumentId,
        action: MarketStatusAction,
        ts_event: int,
        ts_init: int,
        reason: str | None,
        trading_event: str | None,
        is_trading: bool | None,
        is_quoting: bool | None,
        is_short_sell_restricted: bool | None,
    ) -> None: ...
    @property
    def instrument_id(self) -> InstrumentId: ...
    @property
    def action(self) -> MarketStatusAction: ...
    @property
    def ts_event(self) -> int: ...
    @property
    def ts_init(self) -> int: ...
    @property
    def reason(self) -> str | None: ...
    @property
    def trading_event(self) -> str | None: ...
    @property
    def is_trading(self) -> bool | None: ...
    @property
    def is_quoting(self) -> bool | None: ...
    @property
    def is_short_sell_restricted(self) -> bool | None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> InstrumentStatus: ...

# Enums

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
    BINARY_OPTION = "BINARY_OPTION"

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
    @classmethod
    def from_str(cls, value: str) -> CurrencyType: ...

class InstrumentCloseType(Enum):
    END_OF_SESSION = "END_OF_SESSION"
    CONTRACT_EXPIRED = "CONTRACT_EXPIRED"

class LiquiditySide(Enum):
    MAKER = "MAKER"
    TAKER = "TAKER"
    NO_LIQUIDITY_SIDE = "NO_LIQUIDITY_SIDE"

class MarketStatus(Enum):
    OPEN = "OPEN"
    CLOSED = "CLOSED"
    PAUSED = "PAUSED"
    SUSPENDED = "SUSPENDED"
    NOT_AVAILABLE = "NOT_AVAILABLE"

class MarketStatusAction(Enum):
    NONE = "NONE"
    PRE_OPEN = "PRE_OPEN"
    PRE_CROSS = "PRE_CROSS"
    QUOTING = "QUOTING"
    CROSS = "CROSS"
    ROTATION = "ROTATION"
    NEW_PRICE_INDICATION = "NEW_PRICE_INDICATION"
    TRADING = "TRADING"
    HALT = "HALT"
    PAUSE = "PAUSE"
    SUSPEND = "SUSPEND"
    PRE_CLOSE = "PRE_CLOSE"
    CLOSE = "CLOSE"
    POST_CLOSE = "POST_CLOSE"
    SHORT_SELL_RESTRICTION_CHANGE = "SHORT_SELL_RESTRICTION_CHANGE"
    NOT_AVAILABLE_FOR_TRADING = "NOT_AVAILABLE_FOR_TRADING"

class OmsType(Enum):
    UNSPECIFIED = "UNSPECIFIED"
    NETTING = "NETTING"
    HEDGING = "HEDGING"

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

class RecordFlag(Enum):
    F_LAST = "F_LAST"
    F_TOB = "F_TOB"
    F_SNAPSHOT = "F_SNAPSHOT"
    F_MBP = "F_MBP"

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
    WEIGHTED = "WEIGHTED"
    VARIABLE_INDEX_DYNAMIC = "VARIABLE_INDEX_DYNAMIC"

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

# Identifiers

class AccountId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> AccountId: ...
    def value(self) -> str: ...

class ClientId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> ClientId: ...
    def value(self) -> str: ...

class ClientOrderId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> ClientOrderId: ...
    @property
    def value(self) -> str: ...

class ComponentId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> ComponentId: ...
    def value(self) -> str: ...

class ExecAlgorithmId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> ExecAlgorithmId: ...
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
    @classmethod
    def from_str(cls, value: str) -> OrderListId: ...
    def value(self) -> str: ...

class PositionId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> PositionId: ...
    def value(self) -> str: ...

class StrategyId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> StrategyId: ...
    def value(self) -> str: ...

class Symbol:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> Symbol: ...
    @property
    def value(self) -> str: ...
    @property
    def is_composite(self) -> bool: ...
    @property
    def root(self) -> str: ...
    @property
    def topic(self) -> str: ...

class TradeId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> TradeId: ...
    def value(self) -> str: ...

class TraderId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> TraderId: ...
    def value(self) -> str: ...

class Venue:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> Venue: ...
    def value(self) -> str: ...

class VenueOrderId:
    def __init__(self, value: str) -> None: ...
    @classmethod
    def from_str(cls, value: str) -> VenueOrderId: ...
    def value(self) -> str: ...

# Orders

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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> LimitOrder: ...
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
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> LimitOrder: ...
    def apply(self, event: object) -> None: ...

class LimitIfTouchedOrder:
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
        tags: list[str] | None = None,
    ) -> None: ...
    @classmethod
    def create(cls, init: OrderInitialized) -> LimitIfTouchedOrder: ...
    def apply(self, event: object) -> None: ...

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
        time_in_force: TimeInForce,
        reduce_only: bool,
        quote_quantity: bool,
        contingency_type: ContingencyType | None = None,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, str] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: list[str] | None = None,
    ) -> None: ...
    @classmethod
    def create(cls, init: OrderInitialized) -> MarketOrder: ...
    def to_dict(self) -> dict[str, str]: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> MarketOrder: ...
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
    def apply(self, event: object) -> None: ...

class MarketToLimitOrder:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        time_in_force: TimeInForce,
        post_only: bool,
        reduce_only: bool,
        quote_quantity: bool,
        init_id: UUID4,
        ts_init: int,
        expire_time: int | None = None,
        display_qty: Quantity | None = None,
        contingency_type: ContingencyType | None = None,
        order_list_id: OrderListId | None = None,
        linked_order_ids: list[ClientOrderId] | None = None,
        parent_order_id: ClientOrderId | None = None,
        exec_algorithm_id: ExecAlgorithmId | None = None,
        exec_algorithm_params: dict[str, str] | None = None,
        exec_spawn_id: ClientOrderId | None = None,
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> MarketToLimitOrder: ...
    def apply(self, event: object) -> None: ...

class MarketIfTouchedOrder:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> MarketIfTouchedOrder: ...
    def apply(self, event: object) -> None: ...

class StopLimitOrder:
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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> StopLimitOrder: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> StopLimitOrder: ...
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
    def trigger_price(self) -> Price: ...
    @property
    def trigger_type(self) -> TriggerType: ...
    @property
    def time_in_force(self) -> TimeInForce: ...
    @property
    def is_post_only(self) -> bool: ...
    @property
    def is_reduce_only(self) -> bool: ...
    @property
    def is_quote_quantity(self) -> bool: ...
    @property
    def is_passive(self) -> bool: ...
    @property
    def is_aggressive(self) -> bool: ...
    @property
    def is_closed(self) -> bool: ...
    @property
    def is_open(self) -> bool: ...
    @property
    def status(self) -> OrderStatus: ...
    @property
    def has_price(self) -> bool: ...
    @property
    def has_trigger_price(self) -> bool: ...
    @property
    def expire_time(self) -> int | None: ...
    def apply(self, event: object) -> None: ...

class StopMarketOrder:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType,
        time_in_force: TimeInForce,
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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> StopMarketOrder: ...
    def apply(self, event: object) -> None: ...

class TrailingStopLimitOrder:
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
        limit_offset: Price,
        trailing_offset: Price,
        trailing_offset_type: TrailingOffsetType,
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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> TrailingStopLimitOrder: ...
    def apply(self, event: object) -> None: ...

class TrailingStopMarketOrder:
    def __init__(
        self,
        trader_id: TraderId,
        strategy_id: StrategyId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        order_side: OrderSide,
        quantity: Quantity,
        trigger_price: Price,
        trigger_type: TriggerType,
        trailing_offset: Price,
        trailing_offset_type: TrailingOffsetType,
        time_in_force: TimeInForce,
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
        tags: list[str] | None = None,
    ): ...
    @classmethod
    def create(cls, init: OrderInitialized) -> TrailingStopMarketOrder: ...
    def apply(self, event: object) -> None: ...

Order: TypeAlias = Union[
    LimitOrder,
    LimitIfTouchedOrder,
    MarketOrder,
    MarketToLimitOrder,
    MarketIfTouchedOrder,
    StopLimitOrder,
    StopMarketOrder,
    TrailingStopLimitOrder,
    TrailingStopMarketOrder,
]

# Objects

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
        base_currency: Currency | None,
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
    @property
    def account_id(self) -> AccountId: ...
    @property
    def account_type(self) -> AccountType: ...
    @property
    def base_currency(self) -> Currency | None: ...
    @property
    def balances(self) -> list[AccountBalance]: ...
    @property
    def margins(self) -> list[MarginBalance]: ...

# Instruments

class BettingInstrument:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        event_type_id: int,
        event_type_name: str,
        competition_id: int,
        competition_name: str,
        event_id: int,
        event_name: str,
        event_country_code: str,
        event_open_date: int,
        betting_type: str,
        market_id: str,
        market_name: str,
        market_type: str,
        market_start_time: int,
        selection_id: int,
        selection_name: str,
        selection_handicap: float,
        currency: Currency,
        price_precision: int,
        size_precision: int,
        price_increment: Price,
        size_increment: Quantity,
        maker_fee: Decimal,
        taker_fee: Decimal,
        ts_event: int,
        ts_init: int,
        outcome: str | None = None,
        description: str | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> BettingInstrument: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def asset_class(self) -> AssetClass: ...
    @property
    def currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    def to_dict(self) -> dict[str, Any]: ...

class BinaryOption:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        asset_class: AssetClass,
        currency: Currency,
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
        outcome: str | None = None,
        description: str | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_notional: Money | None = None,
        min_notional: Money | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> BinaryOption: ...
    @property
    def id(self) -> InstrumentId: ...
    @property
    def raw_symbol(self) -> Symbol: ...
    @property
    def asset_class(self) -> AssetClass: ...
    @property
    def currency(self) -> Currency: ...
    @property
    def price_precision(self) -> int: ...
    @property
    def size_precision(self) -> int: ...
    @property
    def price_increment(self) -> Price: ...
    @property
    def size_increment(self) -> Quantity: ...
    @property
    def outcome(self) -> str | None: ...
    @property
    def description(self) -> str | None: ...
    def to_dict(self) -> dict[str, Any]: ...

class CryptoFuture:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        underlying: Currency,
        quote_currency: Currency,
        settlement_currency: Currency,
        is_inverse: bool,
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
        raw_symbol: Symbol,
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
        strike_price: Price,
        currency: Currency,
        activation_ns: int,
        expiration_ns: int,
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
    ) -> None: ...
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

# Events

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
    ) -> None: ...
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
    def order_type(self) -> OrderType: ...
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
        tags: list[str] | None = None,
    ) -> None: ...
    @classmethod
    def from_dict(cls, values: dict[str, str]) -> OrderInitialized: ...
    def to_dict(self) -> dict[str, str]: ...
    @property
    def order_type(self) -> OrderType: ...

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
    @property
    def order_type(self) -> str: ...

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
    ) -> None: ...
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
    ) -> None: ...
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

class PositionSnapshot:
    @classmethod
    def from_dict(cls, values: dict[str, Any]) -> PositionSnapshot: ...

# OrderBook

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

class OrderBook:
    def __init__(
        self,
        instrument_id: InstrumentId,
        book_type: BookType,
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
    def add(self, order: BookOrder, flags: int, sequence: int, ts_event: int) -> None: ...
    def update(self, order: BookOrder, flags: int, sequence: int, ts_event: int) -> None: ...
    def delete(self, order: BookOrder, flags: int, sequence: int, ts_event: int) -> None: ...
    def clear(self, sequence: int, ts_event: int) -> None: ...
    def clear_bids(self, sequence: int, ts_event: int) -> None: ...
    def clear_asks(self, sequence: int, ts_event: int) -> None: ...
    def apply_delta(self, delta: OrderBookDelta) -> None: ...
    def apply_deltas(self, deltas: OrderBookDeltas) -> None: ...
    def apply_depth(self, depth: OrderBookDepth10) -> None: ...
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

def update_book_with_quote_tick(book: OrderBook, quote: QuoteTick) -> None: ...
def update_book_with_trade_tick(book: OrderBook, trade: TradeTick) -> None: ...

###################################################################################################
# Infrastructure
###################################################################################################

class BusMessage:
    @property
    def topic(self) -> str: ...
    @property
    def payload(self) -> bytes: ...

class RedisMessageBusDatabase:
    def __init__(
        self,
        trader_id: TraderId,
        instance_id: UUID4,
        config_json: bytes,  # TODO: Standardize this back to `dict[str, Any]`
    ) -> None: ...
    def publish(self, topic: str, payload: bytes) -> None: ...
    def close(self) -> None: ...

class RedisCacheDatabase:
    def __init__(
        self,
        trader_id: TraderId,
        instance_id: UUID4,
        config: dict[str, Any],
    ) -> None: ...

class PostgresCacheDatabase:
    @classmethod
    def connect(
        cls,
        host: str | None = None,
        port: int | None = None,
        username: str | None = None,
        password: str | None = None,
        database: str | None = None,
    ) -> PostgresCacheDatabase: ...
    def close(self) -> None: ...
    def flush_db(self) -> None: ...
    def load(self) -> dict[str, str]: ...
    def load_currency(self, code: str) -> Currency | None: ...
    def load_currencies(self) -> list[Currency]: ...
    def load_instrument(self, instrument_id: InstrumentId) -> Instrument | None: ...
    def load_instruments(self) -> list[Instrument]: ...
    def load_order(self, client_order_id: ClientOrderId) -> Order | None: ...
    def load_account(self, account_id: AccountId) -> Account | None: ...
    def load_trades(self, instrument_id: InstrumentId) -> list[TradeTick]: ...
    def load_quotes(self, instrument_id: InstrumentId) -> list[QuoteTick]: ...
    def load_bars(self, instrument_id: InstrumentId) -> list[Bar]: ...
    def load_signals(self, name: str) -> list[Signal]: ...
    def load_custom_data(self, data_type: DataType) -> list[CustomData]: ...
    def add(self, key: str, value: bytes) -> None: ...
    def add_currency(self, currency: Currency) -> None: ...
    def add_instrument(self, instrument: object) -> None: ...
    def add_order(self, order: object) -> None: ...
    def add_position_snapshot(self, snapshot: PositionSnapshot) -> None: ...
    def add_account(self, account: object) -> None: ...
    def add_trade(self, trade: TradeTick) -> None: ...
    def add_quote(self, quote: QuoteTick) -> None: ...
    def add_bar(self, bar: Bar) -> None: ...
    def add_signal(self, signal: Signal) -> None: ...
    def add_custom_data(self, data: CustomData) -> None: ...
    def update_order(self, order: object) -> None: ...
    def update_account(self, account: Account) -> None: ...

###################################################################################################
# Network
###################################################################################################

class HttpError(Exception):
    ...

class HttpTimeoutError(Exception):
    ...

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
        timeout_secs: int | None = None,
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

class WebSocketClientError(Exception):
    ...

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
        keyed_quotas: list[tuple[str, Quota]] = [],
        default_quota: Quota | None = None,
    ) -> Awaitable[WebSocketClient]: ...
    def disconnect(self) -> Awaitable[None]: ...
    def is_alive(self) -> bool: ...
    def send(self, data: bytes, keys: list[str] | None = None) -> Awaitable[None]: ...
    def send_text(self, data: bytes, keys: list[str] | None = None) -> Awaitable[None]: ...
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
    def disconnect(self) -> Awaitable[None]: ...
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
    def __init__(self, chunk_size: int = 10_000) -> None: ...
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
# Serialization
###################################################################################################

def get_arrow_schema_map(data_cls: type) -> dict[str, str]: ...
def pyobjects_to_arrow_record_batch_bytes(data: list[Data]) -> bytes: ...
def order_book_deltas_to_arrow_record_batch_bytes(data: list[OrderBookDelta]) -> bytes: ...
def order_book_depth10_to_arrow_record_batch_bytes(data: list[OrderBookDepth10]) -> bytes: ...
def quote_ticks_to_arrow_record_batch_bytes(data: list[QuoteTick]) -> bytes: ...
def trade_ticks_to_arrow_record_batch_bytes(data: list[TradeTick]) -> bytes: ...
def bars_to_arrow_record_batch_bytes(data: list[Bar]) -> bytes: ...

###################################################################################################
# Indicators
###################################################################################################

class AdaptiveMovingAverage:
    def __init__(
        self,
        period_efficiency_ratio: int,
        period_fast: int,
        period_slow: int,
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
    def update_raw(self, value: float) -> None: ...
    def reset(self) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...

class SimpleMovingAverage:
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

class LinearRegression:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def slope(self) -> float: ...
    @property
    def intercept(self) -> float: ...
    @property
    def degree(self) -> float: ...
    @property
    def cfo(self) -> float: ...
    @property
    def r2(self) -> float: ...
    @property
    def value(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    def update_raw(self, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class DoubleExponentialMovingAverage:
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
    def update_raw(self, value: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class HullMovingAverage:
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
    def update_raw(self, close: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class VolumeWeightedAveragePrice:
    def __init__(
        self,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, price: float, volume: float, timestamp: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class PsychologicalLine:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class VerticalHorizontalFilter:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, close: float) -> None: ...
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

class KlingerVolumeOscillator:
    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        signal_period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def signal_period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float, volume: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class DirectionalMovement:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def pos(self) -> float: ...
    @property
    def neg(self) -> float: ...
    def update_raw(self, high: float, low: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class ArcherMovingAveragesTrends:
    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        signal_period: int,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def signal_period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def long_run(self) -> bool: ...
    @property
    def short_run(self) -> bool: ...
    def update_raw(self, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class Swings:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def direction(self) -> float: ...
    @property
    def changed(self) -> bool: ...
    @property
    def high_datetime(self) -> float: ...
    @property
    def low_datetime(self) -> float: ...
    @property
    def high_price(self) -> float: ...
    @property
    def low_price(self) -> float: ...
    @property
    def duration(self) -> int: ...
    @property
    def since_high(self) -> int: ...
    @property
    def since_low(self) -> int: ...
    @property
    def length(self) -> int: ...
    def update_raw(self, high: float, low: float, timestamp: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class BollingerBands:
    def __init__(
        self,
        period: int,
        k: float,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def k(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def upper(self) -> float: ...
    @property
    def middle(self) -> float: ...
    @property
    def lower(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class Stochastics:
    def __init__(
        self,
        period_k: int,
        period_d: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period_k(self) -> int: ...
    @property
    def period_d(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value_k(self) -> float: ...
    @property
    def value_d(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class VolatilityRatio:
    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        use_previous: bool,
        value_floor: float,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def use_previous(self) -> bool: ...
    @property
    def value_floor(self) -> float: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class Pressure:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
        atr_floor: float = 0.0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def value_cumulative(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float, volume: float) -> None: ...
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

class Bias:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
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

class EfficiencyRatio:
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
    def value(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    def update_raw(self, close: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class RelativeStrengthIndex:
    def __init__(
        self,
        period: int,
        ma_type: MovingAverageType = ...,
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
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class RelativeVolatilityIndex:
    def __init__(
        self,
        period: int,
        scalar: float,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def scalar(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class RateOfChange:
    def __init__(
        self,
        period: int,
        use_log: bool,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def use_log(self) -> bool: ...
    @property
    def value(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    def update_raw(self, price: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class MovingAverageConvergenceDivergence:
    def __init__(
        self,
        fast_period: int,
        slow_period: int,
        ma_type: MovingAverageType = ...,
        price_type: PriceType | None = None,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def count(self) -> int: ...
    @property
    def fast_period(self) -> int: ...
    @property
    def slow_period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, close: float) -> None: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def handle_trade_tick(self, trade: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class OnBalanceVolume:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, open: float, close: float, volume: float) -> None: ...
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

class CommodityChannelIndex:
    def __init__(
        self,
        period: int,
        scalar: float,
        ma_type: MovingAverageType = ...,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def scalar(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class DonchianChannel:
    def __init__(
        self,
        period: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def upper(self) -> float: ...
    @property
    def middle(self) -> float: ...
    @property
    def lower(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    def update_raw(self, high: float, low: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class KeltnerChannel:
    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = ...,
        ma_type_atr: MovingAverageType = ...,
        use_previous: bool = True,
        atr_floor: float = 0.0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def k_multiplier(self) -> float: ...
    @property
    def use_previous(self) -> bool: ...
    @property
    def atr_floor(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def upper(self) -> float: ...
    @property
    def middle(self) -> float: ...
    @property
    def lower(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class FuzzyCandle:
    def __init__(
        self,
        direction: CandleDirection,
        size: CandleSize,
        body_size: CandleBodySize,
        upper_wick_size: CandleWickSize,
        lower_wick_size: CandleWickSize,
    ) -> None: ...
    @property
    def direction(self) -> CandleDirection: ...
    @property
    def size(self) -> CandleBodySize: ...
    @property
    def body_size(self) -> CandleBodySize: ...
    @property
    def upper_wick_size(self) -> CandleWickSize: ...
    @property
    def lower_wick_size(self) -> CandleWickSize: ...

class FuzzyCandlesticks:
    def __init__(
        self,
        period: int,
        threshold1: float,
        threshold2: float,
        threshold3: float,
        threshold4: float,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def threshold1(self) -> float: ...
    @property
    def threshold2(self) -> float: ...
    @property
    def threshold3(self) -> float: ...
    @property
    def threshold4(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> FuzzyCandle: ...
    @property
    def vector(self) -> list[int]: ...
    def update_raw(self, open: float, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

# Fuzzy Enums
class CandleBodySize(Enum):
    NONE = 0
    SMALL = 1
    MEDIUM = 2
    LARGE = 3
    TREND = 4

class CandleDirection(Enum):
    BULL = 1
    NONE = 0
    BEAR = -1

class CandleSize(Enum):
    NONE = 0
    VERY_SMALL = 1
    SMALL = 2
    MEDIUM = 3
    LARGE = 4
    VERY_LARGE = 5
    EXTREMELY_LARGE = 6

class CandleWickSize(Enum):
    NONE = 0
    SMALL = 1
    MEDIUM = 2
    LARGE = 3

class SpreadAnalyzer:
    def __init__(
        self,
        instrument_id: InstrumentId,
        capacity: int,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def capacity(self) -> int: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def current(self) -> float: ...
    @property
    def average(self) -> float: ...
    def handle_quote_tick(self, quote: QuoteTick) -> None: ...
    def reset(self) -> None: ...

class KeltnerPosition:
    def __init__(
        self,
        period: int,
        k_multiplier: float,
        ma_type: MovingAverageType = ...,
        ma_type_atr: MovingAverageType = ...,
        use_previous: bool = True,
        atr_floor: float = 0.0,
    ) -> None: ...
    @property
    def name(self) -> str: ...
    @property
    def period(self) -> int: ...
    @property
    def k_multiplier(self) -> float: ...
    @property
    def use_previous(self) -> bool: ...
    @property
    def atr_floor(self) -> float: ...
    @property
    def initialized(self) -> bool: ...
    @property
    def has_inputs(self) -> bool: ...
    @property
    def value(self) -> float: ...
    def update_raw(self, high: float, low: float, close: float) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...

class WeightedMovingAverage:
    def __init__(
        self,
        period: int,
        weights: list[float],
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
    def handle_book(self, book: OrderBook) -> None: ...
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
        publishers_filepath: PathLike[str] | str,
    ) -> None: ...
    def load_publishers(self, filepath: PathLike[str] | str) -> None: ...
    def get_publishers(self) -> dict[int, DatabentoPublisher]: ...
    def get_dataset_for_venue(self, venue: Venue) -> str: ...
    def schema_for_file(self, filepath: str) -> str: ...
    def load_instruments(self, filepath: str) -> list[Instrument]: ...
    def load_order_book_deltas(self, filepath: str, instrument_id: InstrumentId | None) -> list[OrderBookDelta]: ...
    def load_order_book_deltas_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> object: ...
    def load_order_book_depth10(self, filepath: str, instrument_id: InstrumentId | None) -> list[OrderBookDepth10]: ...
    def load_order_book_depth10_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None) -> object: ...
    def load_quotes(self, filepath: str, instrument_id: InstrumentId | None) -> list[QuoteTick]: ...
    def load_quotes_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None, include_trades: bool | None) -> object: ...
    def load_bbo_quotes(self, filepath: str, instrument_id: InstrumentId | None) -> list[QuoteTick]: ...
    def load_bbo_quotes_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None) -> object: ...
    def load_trades(self, filepath: str, instrument_id: InstrumentId | None) -> list[TradeTick]: ...
    def load_trades_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None) -> object: ...
    def load_bars(self, filepath: str, instrument_id: InstrumentId | None) -> list[Bar]: ...
    def load_bars_as_pycapsule(self, filepath: str, instrument_id: InstrumentId | None) -> object: ...
    def load_status(self, filepath: str, instrument_id: InstrumentId | None) -> list[InstrumentStatus]: ...
    def load_imbalance(self, filepath: str, instrument_id: InstrumentId | None) -> list[DatabentoImbalance]: ...
    def load_statistics(self, filepath: str, instrument_id: InstrumentId | None) -> list[DatabentoStatistics]: ...

class DatabentoHistoricalClient:
    def __init__(
        self,
        key: str,
        publishers_filepath: str,
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
    async def get_range_status(
        self,
        dataset: str,
        symbols: list[str],
        start: int,
        end: int | None = None,
        limit: int | None = None,
    ) -> list[InstrumentStatus]: ...

class DatabentoLiveClient:
    def __init__(
        self,
        key: str,
        dataset: str,
        publishers_filepath: str,
    ) -> None: ...
    @property
    def key(self) -> str: ...
    @property
    def dataset(self) -> str: ...
    def is_running(self) -> bool: ...
    def is_closed(self) -> bool: ...
    def subscribe(
        self,
        schema: str,
        symbols: list[str],
        stype_in: str | None = None,
        start: int | None = None,
        snapshot: bool | None = False,
    ) -> dict[str, str]: ...
    def start(
        self,
        callback: Callable,
        callback_pyo3: Callable,
    ) -> Awaitable[None]: ...
    def close(self) -> None: ...

# Tardis

def tardis_exchange_from_venue_str(venue_str: str) -> list[str]: ...

def load_tardis_deltas(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> list[OrderBookDelta]: ...  # noqa
def load_tardis_depth10_from_snapshot5(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> list[OrderBookDepth10]: ...  # noqa
def load_tardis_depth10_from_snapshot25(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> list[OrderBookDepth10]: ...  # noqa
def load_tardis_quotes(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> list[QuoteTick]: ...  # noqa
def load_tardis_trades(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> list[TradeTick]: ...  # noqa
def load_tardis_deltas_as_pycapsule(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> object: ...  # noqa
def load_tardis_depth10_from_snapshot5_as_pycapsule(filepath: str, price_precision: int, size_precision: int,  instrument_id: InstrumentId | None, limit: int | None = None) -> object: ...  # noqa
def load_tardis_depth10_from_snapshot25_as_pycapsule(filepath: str, price_precision: int, size_precision: int,  instrument_id: InstrumentId | None, limit: int | None = None) -> object: ...  # noqa
def load_tardis_quotes_as_pycapsule(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> object: ...  # noqa
def load_tardis_trades_as_pycapsule(filepath: str, price_precision: int, size_precision: int, instrument_id: InstrumentId | None, limit: int | None = None) -> object: ...  # noqa

class TardisHttpClient:
    def __init__(
        self,
        api_key: str | None = None,
        base_url: str | None = None,
        timeout_secs: int = 60,
        normalize_symbols: bool = True,
    ) -> None: ...
    async def instrument(self, exchange: str, symbol: str) -> Instrument: ...
    async def instruments(self, exchange: str) -> list[Instrument]: ...

class ReplayNormalizedRequestOptions:
    @classmethod
    def from_json(cls, data: bytes) -> ReplayNormalizedRequestOptions: ...
    @classmethod
    def from_json_array(cls, data: bytes) -> list[ReplayNormalizedRequestOptions]: ...

class StreamNormalizedRequestOptions:
    @classmethod
    def from_json(cls, data: bytes) -> StreamNormalizedRequestOptions: ...
    @classmethod
    def from_json_array(cls, data: bytes) -> list[StreamNormalizedRequestOptions]: ...

class TardisMachineClient:
    def __init__(self, base_url: str | None = None, normalize_symbols: bool = True) -> None: ...
    def is_closed(self) -> bool: ...
    def close(self) -> None: ...
    def replay(self, options: list[ReplayNormalizedRequestOptions], callback: Callable) -> None: ...
    def stream(self, options: list[StreamNormalizedRequestOptions], callback: Callable) -> None: ...

async def run_tardis_machine_replay(config_filepath: str, output_path: str | None = None) -> None: ...

# Greeks

class BlackScholesGreeksResult:
    price: float
    delta: float
    gamma: float
    vega: float
    theta: float

class ImplyVolAndGreeksResult:
    vol: float
    price: float
    delta: float
    gamma: float
    vega: float
    theta: float


def black_scholes_greeks(
    s: float,
    r: float,
    b: float,
    sigma: float,
    is_call: bool,
    k: float,
    t: float,
    multiplier: float,
) -> BlackScholesGreeksResult:
    """
    Calculate the Black-Scholes Greeks for a given option contract.

    Parameters
    ----------
    s : float
        The current price of the underlying asset.
    r : float
        The risk-free interest rate.
    b : float
        The cost of carry of the underlying asset.
    sigma : float
        The volatility of the underlying asset.
    is_call : bool
        Whether the option is a call (True) or a put (False).
    k : float
        The strike price of the option.
    t : float
        The time to expiration of the option in years.
    multiplier : float
        The multiplier for the option contract.

    Returns
    -------
    BlackScholesGreeksResult
        A named tuple containing the calculated option price, delta, gamma, vega, and theta.
    """


def imply_vol(
    s: float,
    r: float,
    b: float,
    is_call: bool,
    k: float,
    t: float,
    price: float,
) -> float:
    """
    Calculate the implied volatility and Greeks for an option contract.

    Parameters
    ----------
    s : float
        The current price of the underlying asset.
    r : float
        The risk-free interest rate.
    b : float
        The cost of carry of the underlying asset.
    is_call : bool
        Whether the option is a call (True) or a put (False).
    k : float
        The strike price of the option.
    t : float
        The time to expiration of the option in years.
    price : float
        The current market price of the option.
    multiplier : float
        The multiplier for the option contract.

    Returns
    -------
    float
        An implied volatility value.
    """


def imply_vol_and_greeks(
    s: float,
    r: float,
    b: float,
    is_call: bool,
    k: float,
    t: float,
    price: float,
    multiplier: float,
) -> ImplyVolAndGreeksResult :
    """
    Calculate the implied volatility and Greeks for an option contract.

    Parameters
    ----------
    s : float
        The current price of the underlying asset.
    r : float
        The risk-free interest rate.
    b : float
        The cost of carry of the underlying asset.
    is_call : bool
        Whether the option is a call (True) or a put (False).
    k : float
        The strike price of the option.
    t : float
        The time to expiration of the option in years.
    price : float
        The current market price of the option.
    multiplier : float
        The multiplier for the option contract.

    Returns
    -------
    ImplyVolAndGreeksResult
        A named tuple containing the calculated implied volatility, option price, delta, gamma, vega, and theta
    """


class GreeksData(Data):
    instrument_id: InstrumentId
    is_call: bool
    strike: float
    expiry: int
    forward: float
    expiry_in_years: float
    interest_rate: float
    vol: float
    price: float
    delta: float
    gamma: float
    vega: float
    theta: float
    quantity: float
    itm_prob: float

    def __init__(
        self,
        ts_event: int = 0,
        ts_init: int = 0,
        instrument_id: InstrumentId = ...,
        is_call: bool = True,
        strike: float = 0.0,
        expiry: int = 0,
        forward: float = 0.0,
        expiry_in_years: float = 0.0,
        interest_rate: float = 0.0,
        vol: float = 0.0,
        price: float = 0.0,
        delta: float = 0.0,
        gamma: float = 0.0,
        vega: float = 0.0,
        theta: float = 0.0,
        quantity: float = 0.0,
        itm_prob: float = 0.0,
    ): ...

    @classmethod
    def from_delta(cls, instrument_id: InstrumentId, delta: float) -> GreeksData: ...


class PortfolioGreeks(Data):
    delta: float
    gamma: float
    vega: float
    theta: float

    def __init__(
        self,
        ts_event: int = 0,
        ts_init: int = 0,
        delta: float = 0.0,
        gamma: float = 0.0,
        vega: float = 0.0,
        theta: float = 0.0,
    ): ...


class InterestRateData(Data):
    curve_name: str
    interest_rate: float

    def __init__(
        self,
        ts_event: int = 0,
        ts_init: int = 0,
        curve_name: str = "USD",
        interest_rate: float = 0.05,
    ): ...


class InterestRateCurveData(Data):
    curve_name: str
    tenors: np.ndarray
    interest_rates: np.ndarray

    def __init__(
        self,
        ts_event: int,
        ts_init: int,
        curve_name: str,
        tenors: np.ndarray,
        interest_rates: np.ndarray,
    ): ...

###################################################################################################
# Test Kit
###################################################################################################

def ensure_file_exists_or_download_http(filepath: str, url: str, checksums: str | None = None): ...
