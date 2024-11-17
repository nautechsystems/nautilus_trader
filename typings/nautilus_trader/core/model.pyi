# Constants
DEPTH10_LEN: int
TRADE_ID_LEN: int
FIXED_PRECISION: int
FIXED_SCALAR: float
MONEY_MAX: float
MONEY_MIN: float
PRICE_UNDEF: int
PRICE_ERROR: int
PRICE_MAX: float
PRICE_MIN: float
QUANTITY_UNDEF: int
QUANTITY_MAX: float
QUANTITY_MIN: float

# Enums
class AccountType:
    CASH: AccountType
    MARGIN: AccountType
    BETTING: AccountType

class AggregationSource:
    EXTERNAL: AggregationSource
    INTERNAL: AggregationSource

class AggressorSide:
    NO_AGGRESSOR: AggressorSide
    BUYER: AggressorSide
    SELLER: AggressorSide

class AssetClass:
    FX: AssetClass
    EQUITY: AssetClass
    COMMODITY: AssetClass
    DEBT: AssetClass
    INDEX: AssetClass
    CRYPTOCURRENCY: AssetClass
    ALTERNATIVE: AssetClass

class BookAction:
    ADD: BookAction
    UPDATE: BookAction
    DELETE: BookAction
    CLEAR: BookAction

class BookType:
    L1_MBP: BookType
    L2_MBP: BookType
    L3_MBO: BookType

class ContingencyType:
    NO_CONTINGENCY: ContingencyType
    OCO: ContingencyType
    OTO: ContingencyType
    OUO: ContingencyType

class CurrencyType:
    CRYPTO: CurrencyType
    FIAT: CurrencyType
    COMMODITY_BACKED: CurrencyType

class InstrumentClass:
    SPOT: InstrumentClass
    SWAP: InstrumentClass
    FUTURE: InstrumentClass
    FUTURE_SPREAD: InstrumentClass
    FORWARD: InstrumentClass
    CFD: InstrumentClass
    BOND: InstrumentClass
    OPTION: InstrumentClass
    OPTION_SPREAD: InstrumentClass
    WARRANT: InstrumentClass
    SPORTS_BETTING: InstrumentClass
    BINARY_OPTION: InstrumentClass

class InstrumentCloseType:
    END_OF_SESSION: InstrumentCloseType
    CONTRACT_EXPIRED: InstrumentCloseType

class LiquiditySide:
    NO_LIQUIDITY_SIDE: LiquiditySide
    MAKER: LiquiditySide
    TAKER: LiquiditySide

class MarketStatus:
    OPEN: MarketStatus
    CLOSED: MarketStatus
    PAUSED: MarketStatus
    SUSPENDED: MarketStatus
    NOT_AVAILABLE: MarketStatus

class MarketStatusAction:
    NONE: MarketStatusAction
    PRE_OPEN: MarketStatusAction
    PRE_CROSS: MarketStatusAction
    QUOTING: MarketStatusAction
    CROSS: MarketStatusAction
    ROTATION: MarketStatusAction
    NEW_PRICE_INDICATION: MarketStatusAction
    TRADING: MarketStatusAction
    HALT: MarketStatusAction
    PAUSE: MarketStatusAction
    SUSPEND: MarketStatusAction
    PRE_CLOSE: MarketStatusAction
    CLOSE: MarketStatusAction
    POST_CLOSE: MarketStatusAction
    SHORT_SELL_RESTRICTION_CHANGE: MarketStatusAction
    NOT_AVAILABLE_FOR_TRADING: MarketStatusAction

class OmsType:
    UNSPECIFIED: OmsType
    NETTING: OmsType
    HEDGING: OmsType

class OptionKind:
    CALL: OptionKind
    PUT: OptionKind

class OrderSide:
    NO_ORDER_SIDE: OrderSide
    BUY: OrderSide
    SELL: OrderSide

class OrderStatus:
    INITIALIZED: OrderStatus
    DENIED: OrderStatus
    EMULATED: OrderStatus
    RELEASED: OrderStatus
    SUBMITTED: OrderStatus
    ACCEPTED: OrderStatus
    REJECTED: OrderStatus
    CANCELED: OrderStatus
    EXPIRED: OrderStatus
    TRIGGERED: OrderStatus
    PENDING_UPDATE: OrderStatus
    PENDING_CANCEL: OrderStatus
    PARTIALLY_FILLED: OrderStatus
    FILLED: OrderStatus

class OrderType:
    MARKET: OrderType
    LIMIT: OrderType
    STOP_MARKET: OrderType
    STOP_LIMIT: OrderType
    MARKET_TO_LIMIT: OrderType
    MARKET_IF_TOUCHED: OrderType
    LIMIT_IF_TOUCHED: OrderType
    TRAILING_STOP_MARKET: OrderType
    TRAILING_STOP_LIMIT: OrderType

class PositionSide:
    NO_POSITION_SIDE: PositionSide
    FLAT: PositionSide
    LONG: PositionSide
    SHORT: PositionSide

class PriceType:
    BID: PriceType
    ASK: PriceType
    MID: PriceType
    LAST: PriceType

class RecordFlag:
    F_LAST: RecordFlag
    F_TOB: RecordFlag
    F_SNAPSHOT: RecordFlag
    F_MBP: RecordFlag
    RESERVED_2: RecordFlag
    RESERVED_1: RecordFlag

class TimeInForce:
    GTC: TimeInForce
    IOC: TimeInForce
    FOK: TimeInForce
    GTD: TimeInForce
    DAY: TimeInForce
    AT_THE_OPEN: TimeInForce
    AT_THE_CLOSE: TimeInForce

class TradingState:
    ACTIVE: TradingState
    HALTED: TradingState
    REDUCING: TradingState

class TrailingOffsetType:
    NO_TRAILING_OFFSET: TrailingOffsetType
    PRICE: TrailingOffsetType
    BASIS_POINTS: TrailingOffsetType
    TICKS: TrailingOffsetType
    PRICE_TIER: TrailingOffsetType

class TriggerType:
    NO_TRIGGER: TriggerType
    DEFAULT: TriggerType
    BID_ASK: TriggerType
    LAST_TRADE: TriggerType
    DOUBLE_LAST: TriggerType
    DOUBLE_BID_ASK: TriggerType
    LAST_OR_BID_ASK: TriggerType
    MID_POINT: TriggerType
    MARK_PRICE: TriggerType
    INDEX_PRICE: TriggerType
