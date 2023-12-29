# ruff: noqa: UP007 PYI021 PYI044 PYI053
# fmt: off

import datetime as dt
from collections.abc import Awaitable
from collections.abc import Callable
from decimal import Decimal
from enum import Enum
from typing import Any

from pyarrow import RecordBatch

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

class LogGuard: ...

def set_global_log_collector(
    stdout_level: str | None,
    stderr_level: str | None,
    file_level: tuple[str, str, str] | None,
) -> LogGuard: ...

###################################################################################################
# Model
###################################################################################################

### Data types

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
    @staticmethod
    def get_fields() -> dict[str, str]: ...

class BookOrder: ...

class OrderBookDelta:
    @staticmethod
    def get_fields() -> dict[str, str]: ...

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
    EQUITY = "EQUITY"
    COMMODITY = "COMMODITY"
    METAL = "METAL"
    ENERGY = "ENERGY"
    BOND = "BOND"
    INDEX = "INDEX"
    CRYPTO_CURRENCY = "CRYPTO_CURRENCY"
    SPORTS_BETTING = "SPORTS_BETTING"

class AssetType(Enum):
    SPOT = "SPOT"
    SWAP = "SWAP"
    FUTURE = "FUTURE"
    FORWARD = "FORWARD"
    CFD = "CFD"
    OPTION = "OPTION"
    WARRANT = "WARRANT"

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

### Identifiers

class AccountId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class ClientId:
    def __init__(self, value: str) -> None: ...
    def value(self) -> str: ...

class ClientOrderId:
    def __init__(self, value: str) -> None: ...
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

class LimitOrder: ...
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
    @staticmethod
    def opposite_side(side: OrderSide) -> OrderSide: ...
    @staticmethod
    def closing_side(side: PositionSide) -> OrderSide: ...
    def signed_decimal_qty(self) -> Decimal: ...
    def would_reduce_only(self, side: PositionSide, position_qty: Quantity) -> bool: ...
    def commission(self, currency: Currency) -> Money | None: ...
    def commissions(self) -> dict[Currency, Money]: ...

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
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
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
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
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
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
class Equity:
    def __init__(
        self,
        id: InstrumentId,
        raw_symbol: Symbol,
        isin: str,
        currency: Currency,
        price_precision: int,
        price_increment: Price,
        multiplier: Quantity,
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
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
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
        multiplier: Quantity,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None: ...
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
        margin_init: float,
        margin_maint: float,
        maker_fee: float,
        taker_fee: float,
        ts_event: int,
        ts_init: int,
        lot_size: Quantity | None = None,
        max_quantity: Quantity | None = None,
        min_quantity: Quantity | None = None,
        max_price: Price | None = None,
        min_price: Price | None = None,
    ) -> None : ...
class SyntheticInstrument: ...

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
    def send_text(self, data: str) -> Awaitable[None]: ...
    def send(self, data: bytes) -> Awaitable[None]: ...

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
    QuoteTick = 2
    TradeTick = 3
    Bar = 4

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
    def pyobjects_to_batches_bytes(data: list[Data]) -> bytes: ...
    @staticmethod
    def pyo3_order_book_deltas_to_batches_bytes(data: list[OrderBookDelta]) -> bytes: ...
    @staticmethod
    def pyo3_quote_ticks_to_batches_bytes(data: list[QuoteTick]) -> bytes: ...
    @staticmethod
    def pyo3_trade_ticks_to_batches_bytes(data: list[TradeTick]) -> bytes: ...
    @staticmethod
    def pyo3_bars_to_batches_bytes(data: list[Bar]) -> bytes: ...
    @staticmethod
    def record_batches_to_pybytes(batches: list[RecordBatch], schema: Any) -> bytes: ...

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
    def process_record_batches_bytes(self, data: bytes) -> list[Bar]: ...

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
    def process_record_batches_bytes(self, data: bytes) -> list[OrderBookDelta]: ...

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
    def process_record_batches_bytes(self, data: bytes) -> list[QuoteTick]: ...

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
    def process_record_batches_bytes(self, data: bytes) -> list[TradeTick]: ...


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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
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
    def handle_quote_tick(self, tick: QuoteTick) -> None: ...
    def handle_trade_tick(self, tick: TradeTick) -> None: ...
    def handle_bar(self, bar: Bar) -> None: ...
    def reset(self) -> None: ...
