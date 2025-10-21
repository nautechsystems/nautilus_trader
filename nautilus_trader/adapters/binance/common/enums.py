# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------
"""
Defines Binance common enums.

References
----------
https://binance-docs.github.io/apidocs/spot/en/#public-api-definitions
https://binance-docs.github.io/apidocs/futures/en/#public-endpoints-info

"""

from enum import Enum
from enum import unique

from nautilus_trader.model.data import BarSpecification
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PriceType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.enums import bar_aggregation_to_str
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.orders import Order


@unique
class BinanceKeyType(Enum):
    """
    Represents a Binance private key cryptographic algorithm type.
    """

    HMAC = "HMAC"
    RSA = "RSA"
    ED25519 = "Ed25519"


@unique
class BinanceFuturesPositionSide(Enum):
    """
    Represents a Binance Futures position side.
    """

    BOTH = "BOTH"
    LONG = "LONG"
    SHORT = "SHORT"


@unique
class BinanceRateLimitType(Enum):
    """
    Represents a Binance rate limit type.
    """

    REQUEST_WEIGHT = "REQUEST_WEIGHT"
    ORDERS = "ORDERS"
    RAW_REQUESTS = "RAW_REQUESTS"


@unique
class BinanceRateLimitInterval(Enum):
    """
    Represents a Binance rate limit interval.
    """

    SECOND = "SECOND"
    MINUTE = "MINUTE"
    DAY = "DAY"


@unique
class BinanceKlineInterval(Enum):
    """
    Represents a Binance kline chart interval.
    """

    SECOND_1 = "1s"
    MINUTE_1 = "1m"
    MINUTE_3 = "3m"
    MINUTE_5 = "5m"
    MINUTE_15 = "15m"
    MINUTE_30 = "30m"
    HOUR_1 = "1h"
    HOUR_2 = "2h"
    HOUR_4 = "4h"
    HOUR_6 = "6h"
    HOUR_8 = "8h"
    HOUR_12 = "12h"
    DAY_1 = "1d"
    DAY_3 = "3d"
    WEEK_1 = "1w"
    MONTH_1 = "1M"


@unique
class BinanceExchangeFilterType(Enum):
    """
    Represents a Binance exchange filter type.
    """

    EXCHANGE_MAX_NUM_ORDERS = "EXCHANGE_MAX_NUM_ORDERS"
    EXCHANGE_MAX_NUM_ALGO_ORDERS = "EXCHANGE_MAX_NUM_ALGO_ORDERS"


@unique
class BinanceSymbolFilterType(Enum):
    """
    Represents a Binance symbol filter type.
    """

    PRICE_FILTER = "PRICE_FILTER"
    PERCENT_PRICE = "PERCENT_PRICE"
    PERCENT_PRICE_BY_SIDE = "PERCENT_PRICE_BY_SIDE"
    LOT_SIZE = "LOT_SIZE"
    MIN_NOTIONAL = "MIN_NOTIONAL"
    NOTIONAL = "NOTIONAL"
    ICEBERG_PARTS = "ICEBERG_PARTS"
    MARKET_LOT_SIZE = "MARKET_LOT_SIZE"
    MAX_NUM_ORDERS = "MAX_NUM_ORDERS"
    MAX_NUM_ALGO_ORDERS = "MAX_NUM_ALGO_ORDERS"
    MAX_NUM_ICEBERG_ORDERS = "MAX_NUM_ICEBERG_ORDERS"
    MAX_NUM_ORDER_LISTS = "MAX_NUM_ORDER_LISTS"
    MAX_NUM_ORDER_AMENDS = "MAX_NUM_ORDER_AMENDS"
    MAX_POSITION = "MAX_POSITION"
    TRAILING_DELTA = "TRAILING_DELTA"
    POSITION_RISK_CONTROL = "POSITION_RISK_CONTROL"


@unique
class BinanceAccountType(Enum):
    """
    Represents a Binance account type.
    """

    SPOT = "SPOT"
    MARGIN = "MARGIN"
    ISOLATED_MARGIN = "ISOLATED_MARGIN"
    USDT_FUTURES = "USDT_FUTURES"
    COIN_FUTURES = "COIN_FUTURES"

    @property
    def is_spot(self):
        return self == BinanceAccountType.SPOT

    @property
    def is_margin(self):
        return self in (
            BinanceAccountType.MARGIN,
            BinanceAccountType.ISOLATED_MARGIN,
        )

    @property
    def is_spot_or_margin(self):
        return self in (
            BinanceAccountType.SPOT,
            BinanceAccountType.MARGIN,
            BinanceAccountType.ISOLATED_MARGIN,
        )

    @property
    def is_futures(self) -> bool:
        return self in (
            BinanceAccountType.USDT_FUTURES,
            BinanceAccountType.COIN_FUTURES,
        )


@unique
class BinanceOrderSide(Enum):
    """
    Represents a Binance order side.
    """

    BUY = "BUY"
    SELL = "SELL"


@unique
class BinanceExecutionType(Enum):
    """
    Represents a Binance execution type.
    """

    NEW = "NEW"
    CANCELED = "CANCELED"
    CALCULATED = "CALCULATED"  # Liquidation Execution
    REJECTED = "REJECTED"
    TRADE = "TRADE"
    EXPIRED = "EXPIRED"
    AMENDMENT = "AMENDMENT"
    TRADE_PREVENTION = "TRADE_PREVENTION"


@unique
class BinanceOrderStatus(Enum):
    """
    Represents a Binance order status.
    """

    NEW = "NEW"
    PARTIALLY_FILLED = "PARTIALLY_FILLED"
    FILLED = "FILLED"
    CANCELED = "CANCELED"
    PENDING_CANCEL = "PENDING_CANCEL"
    REJECTED = "REJECTED"
    EXPIRED = "EXPIRED"
    EXPIRED_IN_MATCH = "EXPIRED_IN_MATCH"
    NEW_INSURANCE = "NEW_INSURANCE"  # Liquidation with Insurance Fund
    NEW_ADL = "NEW_ADL"  # Counterparty Liquidation


@unique
class BinanceTimeInForce(Enum):
    """
    Represents a Binance order time in force.
    """

    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    GTX = "GTX"  # FUTURES only, Good Till Crossing (Post Only)
    GTD = "GTD"  # FUTURES only
    GTE_GTC = "GTE_GTC"  # Undocumented


@unique
class BinanceOrderType(Enum):
    """
    Represents a Binance order type.
    """

    LIMIT = "LIMIT"
    MARKET = "MARKET"
    STOP = "STOP"  # FUTURES only
    STOP_LOSS = "STOP_LOSS"  # SPOT/MARGIN only
    STOP_LOSS_LIMIT = "STOP_LOSS_LIMIT"  # SPOT/MARGIN only
    TAKE_PROFIT = "TAKE_PROFIT"
    TAKE_PROFIT_LIMIT = "TAKE_PROFIT_LIMIT"  # SPOT/MARGIN only
    LIMIT_MAKER = "LIMIT_MAKER"  # SPOT/MARGIN only
    STOP_MARKET = "STOP_MARKET"  # FUTURES only
    TAKE_PROFIT_MARKET = "TAKE_PROFIT_MARKET"  # FUTURES only
    TRAILING_STOP_MARKET = "TRAILING_STOP_MARKET"  # FUTURES only
    INSURANCE_FUND = "INSURANCE_FUND"


@unique
class BinanceSecurityType(Enum):
    """
    Represents a Binance endpoint security type.
    """

    NONE = "NONE"
    TRADE = "TRADE"
    MARGIN = "MARGIN"  # SPOT/MARGIN only
    USER_DATA = "USER_DATA"
    USER_STREAM = "USER_STREAM"
    MARKET_DATA = "MARKET_DATA"


@unique
class BinanceNewOrderRespType(Enum):
    """
    Represents a Binance newOrderRespType.
    """

    ACK = "ACK"
    RESULT = "RESULT"
    FULL = "FULL"


@unique
class BinanceErrorCode(Enum):
    """
    Represents a Binance error code (covers futures).
    """

    UNKNOWN = -1000
    DISCONNECTED = -1001
    UNAUTHORIZED = -1002
    TOO_MANY_REQUESTS = -1003
    DUPLICATE_IP = -1004
    NO_SUCH_IP = -1005
    UNEXPECTED_RESP = -1006
    TIMEOUT = -1007
    SERVER_BUSY = -1008
    ERROR_MSG_RECEIVED = -1010
    NON_WHITE_LIST = -1011
    INVALID_MESSAGE = -1013
    UNKNOWN_ORDER_COMPOSITION = -1014
    TOO_MANY_ORDERS = -1015
    SERVICE_SHUTTING_DOWN = -1016
    UNSUPPORTED_OPERATION = -1020
    INVALID_TIMESTAMP = -1021
    INVALID_SIGNATURE = -1022
    START_TIME_GREATER_THAN_END_TIME = -1023
    NOT_FOUND = -1099
    ILLEGAL_CHARS = -1100
    TOO_MANY_PARAMETERS = -1101
    MANDATORY_PARAM_EMPTY_OR_MALFORMED = -1102
    UNKNOWN_PARAM = -1103
    UNREAD_PARAMETERS = -1104
    PARAM_EMPTY = -1105
    PARAM_NOT_REQUIRED = -1106
    BAD_ASSET = -1108
    BAD_ACCOUNT = -1109
    BAD_INSTRUMENT_TYPE = -1110
    BAD_PRECISION = -1111
    NO_DEPTH = -1112
    WITHDRAW_NOT_NEGATIVE = -1113
    TIF_NOT_REQUIRED = -1114
    INVALID_TIF = -1115
    INVALID_ORDER_TYPE = -1116
    INVALID_SIDE = -1117
    EMPTY_NEW_CL_ORD_ID = -1118
    EMPTY_ORG_CL_ORD_ID = -1119
    BAD_INTERVAL = -1120
    BAD_SYMBOL = -1121
    INVALID_SYMBOL_STATUS = -1122
    INVALID_LISTEN_KEY = -1125
    ASSET_NOT_SUPPORTED = -1126
    MORE_THAN_XX_HOURS = -1127
    OPTIONAL_PARAMS_BAD_COMBO = -1128
    ORDER_AMEND_KEEP_PRIORITY_FAILED = -2038
    ORDER_QUERY_DUAL_ID_NOT_FOUND = -2039
    INVALID_PARAMETER = -1130
    INVALID_NEW_ORDER_RESP_TYPE = -1136

    INVALID_CALLBACK_RATE = -2007
    NEW_ORDER_REJECTED = -2010
    CANCEL_REJECTED = -2011
    CANCEL_ALL_FAIL = -2012
    NO_SUCH_ORDER = -2013
    BAD_API_KEY_FMT = -2014
    REJECTED_MBX_KEY = -2015
    NO_TRADING_WINDOW = -2016
    API_KEYS_LOCKED = -2017
    BALANCE_NOT_SUFFICIENT = -2018
    MARGIN_NOT_SUFFICIENT = -2019
    UNABLE_TO_FILL = -2020
    ORDER_WOULD_IMMEDIATELY_TRIGGER = -2021
    REDUCE_ONLY_REJECT = -2022
    USER_IN_LIQUIDATION = -2023
    POSITION_NOT_SUFFICIENT = -2024
    MAX_OPEN_ORDER_EXCEEDED = -2025
    REDUCE_ONLY_ORDER_TYPE_NOT_SUPPORTED = -2026
    MAX_LEVERAGE_RATIO = -2027
    MIN_LEVERAGE_RATIO = -2028

    INVALID_ORDER_STATUS = -4000
    PRICE_LESS_THAN_ZERO = -4001
    PRICE_GREATER_THAN_MAX_PRICE = -4002
    QTY_LESS_THAN_ZERO = -4003
    QTY_LESS_THAN_MIN_QTY = -4004
    QTY_GREATER_THAN_MAX_QTY = -4005
    STOP_PRICE_LESS_THAN_ZERO = -4006
    STOP_PRICE_GREATER_THAN_MAX_PRICE = -4007
    TICK_SIZE_LESS_THAN_ZERO = -4008
    MAX_PRICE_LESS_THAN_MIN_PRICE = -4009
    MAX_QTY_LESS_THAN_MIN_QTY = -4010
    STEP_SIZE_LESS_THAN_ZERO = -4011
    MAX_NUM_ORDERS_LESS_THAN_ZERO = -4012
    PRICE_LESS_THAN_MIN_PRICE = -4013
    PRICE_NOT_INCREASED_BY_TICK_SIZE = -4014
    INVALID_CL_ORD_ID_LEN = -4015
    PRICE_HIGHTER_THAN_MULTIPLIER_UP = -4016  # Binance's official typo (should be HIGHER)
    MULTIPLIER_UP_LESS_THAN_ZERO = -4017
    MULTIPLIER_DOWN_LESS_THAN_ZERO = -4018
    COMPOSITE_SCALE_OVERFLOW = -4019
    TARGET_STRATEGY_INVALID = -4020
    INVALID_DEPTH_LIMIT = -4021
    WRONG_MARKET_STATUS = -4022
    QTY_NOT_INCREASED_BY_STEP_SIZE = -4023
    PRICE_LOWER_THAN_MULTIPLIER_DOWN = -4024
    MULTIPLIER_DECIMAL_LESS_THAN_ZERO = -4025
    COMMISSION_INVALID = -4026
    INVALID_ACCOUNT_TYPE = -4027
    INVALID_LEVERAGE = -4028
    INVALID_TICK_SIZE_PRECISION = -4029
    INVALID_STEP_SIZE_PRECISION = -4030
    INVALID_WORKING_TYPE = -4031
    EXCEED_MAX_CANCEL_ORDER_SIZE = -4032
    INSURANCE_ACCOUNT_NOT_FOUND = -4033
    INVALID_BALANCE_TYPE = -4044
    MAX_STOP_ORDER_EXCEEDED = -4045
    NO_NEED_TO_CHANGE_MARGIN_TYPE = -4046
    THERE_EXISTS_OPEN_ORDERS = -4047
    THERE_EXISTS_QUANTITY = -4048
    ADD_ISOLATED_MARGIN_REJECT = -4049
    CROSS_BALANCE_INSUFFICIENT = -4050
    ISOLATED_BALANCE_INSUFFICIENT = -4051
    NO_NEED_TO_CHANGE_AUTO_ADD_MARGIN = -4052
    AUTO_ADD_CROSSED_MARGIN_REJECT = -4053
    ADD_ISOLATED_MARGIN_NO_POSITION_REJECT = -4054
    AMOUNT_MUST_BE_POSITIVE = -4055
    INVALID_API_KEY_TYPE = -4056
    INVALID_RSA_PUBLIC_KEY = -4057
    MAX_PRICE_TOO_LARGE = -4058
    NO_NEED_TO_CHANGE_POSITION_SIDE = -4059
    INVALID_POSITION_SIDE = -4060
    POSITION_SIDE_NOT_MATCH = -4061
    REDUCE_ONLY_CONFLICT = -4062
    INVALID_OPTIONS_REQUEST_TYPE = -4063
    INVALID_OPTIONS_TIME_FRAME = -4064
    INVALID_OPTIONS_AMOUNT = -4065
    INVALID_OPTIONS_EVENT_TYPE = -4066
    POSITION_SIDE_CHANGE_EXISTS_OPEN_ORDERS = -4067
    POSITION_SIDE_CHANGE_EXISTS_QUANTITY = -4068
    INVALID_OPTIONS_PREMIUM_FEE = -4069
    INVALID_CL_OPTIONS_ID_LEN = -4070
    INVALID_OPTIONS_DIRECTION = -4071
    OPTIONS_PREMIUM_NOT_UPDATE = -4072
    OPTIONS_PREMIUM_INPUT_LESS_THAN_ZERO = -4073
    OPTIONS_AMOUNT_BIGGER_THAN_UPPER = -4074
    OPTIONS_PREMIUM_OUTPUT_ZERO = -4075
    OPTIONS_PREMIUM_TOO_DIFF = -4076
    OPTIONS_PREMIUM_REACH_LIMIT = -4077
    OPTIONS_COMMON_ERROR = -4078
    INVALID_OPTIONS_ID = -4079
    OPTIONS_USER_NOT_FOUND = -4080
    OPTIONS_NOT_FOUND = -4081
    INVALID_BATCH_PLACE_ORDER_SIZE = -4082
    PLACE_BATCH_ORDERS_FAIL = -4083
    UPCOMING_METHOD = -4084
    INVALID_NOTIONAL_LIMIT_COEF = -4085
    INVALID_PRICE_SPREAD_THRESHOLD = -4086
    REDUCE_ONLY_ORDER_PERMISSION = -4087
    NO_PLACE_ORDER_PERMISSION = -4088
    INVALID_CONTRACT_TYPE = -4104
    INVALID_CLIENT_TRAN_ID_LEN = -4114
    DUPLICATED_CLIENT_TRAN_ID = -4115
    REDUCE_ONLY_MARGIN_CHECK_FAILED = -4118
    MARKET_ORDER_REJECT = -4131
    INVALID_ACTIVATION_PRICE = -4135
    QUANTITY_EXISTS_WITH_CLOSE_POSITION = -4137
    REDUCE_ONLY_MUST_BE_TRUE = -4138
    ORDER_TYPE_CANNOT_BE_MKT = -4139
    INVALID_OPENING_POSITION_STATUS = -4140
    SYMBOL_ALREADY_CLOSED = -4141
    STRATEGY_INVALID_TRIGGER_PRICE = -4142
    INVALID_PAIR = -4144
    ISOLATED_LEVERAGE_REJECT_WITH_POSITION = -4161
    MIN_NOTIONAL = -4164
    INVALID_TIME_INTERVAL = -4165
    ISOLATED_REJECT_WITH_JOINT_MARGIN = -4167
    JOINT_MARGIN_REJECT_WITH_ISOLATED = -4168
    JOINT_MARGIN_REJECT_WITH_MB = -4169
    JOINT_MARGIN_REJECT_WITH_OPEN_ORDER = -4170
    NO_NEED_TO_CHANGE_JOINT_MARGIN = -4171
    JOINT_MARGIN_REJECT_WITH_NEGATIVE_BALANCE = -4172
    ISOLATED_REJECT_WITH_JOINT_MARGIN_2 = -4183
    PRICE_LOWER_THAN_STOP_MULTIPLIER_DOWN = -4184
    COOLING_OFF_PERIOD = -4192
    ADJUST_LEVERAGE_KYC_FAILED = -4202
    ADJUST_LEVERAGE_ONE_MONTH_FAILED = -4203
    ADJUST_LEVERAGE_X_DAYS_FAILED = -4205
    ADJUST_LEVERAGE_KYC_LIMIT = -4206
    ADJUST_LEVERAGE_ACCOUNT_SYMBOL_FAILED = -4208
    ADJUST_LEVERAGE_SYMBOL_FAILED = -4209
    STOP_PRICE_HIGHER_THAN_PRICE_MULTIPLIER_LIMIT = -4210
    STOP_PRICE_LOWER_THAN_PRICE_MULTIPLIER_LIMIT = -4211
    TRADING_QUANTITATIVE_RULE = -4400
    COMPLIANCE_RESTRICTION = -4401
    COMPLIANCE_BLACK_SYMBOL_RESTRICTION = -4402
    ADJUST_LEVERAGE_COMPLIANCE_FAILED = -4403

    INVALID_PEG_OFFSET_TYPE = 1211

    FOK_ORDER_REJECT = -5021
    GTX_ORDER_REJECT = -5022
    MOVE_ORDER_NOT_ALLOWED_SYMBOL_REASON = -5024
    LIMIT_ORDER_ONLY = 5025
    EXCEED_MAXIMUM_MODIFY_ORDER_LIMIT = -5026
    SAME_ORDER = -5027
    ME_RECVWINDOW_REJECT = -5028
    INVALID_GOOD_TILL_DATE = -5040


class BinanceEnumParser:
    """
    Provides common parsing methods for enums used by the 'Binance' exchange.

    Warnings
    --------
    This class should not be used directly, but through a concrete subclass.

    """

    def __init__(self) -> None:
        # Construct dictionary hashmaps
        self.ext_to_int_status = {
            BinanceOrderStatus.NEW: OrderStatus.ACCEPTED,
            BinanceOrderStatus.CANCELED: OrderStatus.CANCELED,
            BinanceOrderStatus.PENDING_CANCEL: OrderStatus.PENDING_CANCEL,
            BinanceOrderStatus.REJECTED: OrderStatus.REJECTED,
            BinanceOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            BinanceOrderStatus.FILLED: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_ADL: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_INSURANCE: OrderStatus.FILLED,
            BinanceOrderStatus.EXPIRED: OrderStatus.EXPIRED,
            BinanceOrderStatus.EXPIRED_IN_MATCH: OrderStatus.CANCELED,  # Canceled due self-trade prevention (STP)
        }

        self.ext_to_int_order_side = {
            BinanceOrderSide.BUY: OrderSide.BUY,
            BinanceOrderSide.SELL: OrderSide.SELL,
        }
        self.int_to_ext_order_side = {b: a for a, b in self.ext_to_int_order_side.items()}

        self.ext_to_int_bar_agg = {
            "s": BarAggregation.SECOND,
            "m": BarAggregation.MINUTE,
            "h": BarAggregation.HOUR,
            "d": BarAggregation.DAY,
            "w": BarAggregation.WEEK,
            "M": BarAggregation.MONTH,
        }
        self.int_to_ext_bar_agg = {b: a for a, b in self.ext_to_int_bar_agg.items()}

        self.ext_to_int_time_in_force = {
            BinanceTimeInForce.FOK: TimeInForce.FOK,
            BinanceTimeInForce.GTC: TimeInForce.GTC,
            BinanceTimeInForce.GTX: TimeInForce.GTC,  # Convert GTX to GTC
            BinanceTimeInForce.GTE_GTC: TimeInForce.GTC,  # Undocumented
            BinanceTimeInForce.IOC: TimeInForce.IOC,
            BinanceTimeInForce.GTD: TimeInForce.GTD,
        }
        self.int_to_ext_time_in_force = {
            TimeInForce.GTC: BinanceTimeInForce.GTC,
            TimeInForce.GTD: BinanceTimeInForce.GTD,
            TimeInForce.FOK: BinanceTimeInForce.FOK,
            TimeInForce.IOC: BinanceTimeInForce.IOC,
        }

    def parse_binance_order_side(self, order_side: BinanceOrderSide) -> OrderSide:
        try:
            return self.ext_to_int_order_side[order_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance order side, was {order_side}",  # pragma: no cover
            )

    def parse_internal_order_side(self, order_side: OrderSide) -> BinanceOrderSide:
        try:
            return self.int_to_ext_order_side[order_side]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Nautilus order side, was {order_side}",  # pragma: no cover
            )

    def parse_binance_time_in_force(self, time_in_force: BinanceTimeInForce) -> TimeInForce:
        try:
            return self.ext_to_int_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_internal_time_in_force(self, time_in_force: TimeInForce) -> BinanceTimeInForce:
        try:
            return self.int_to_ext_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Nautilus time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_binance_order_status(self, order_status: BinanceOrderStatus) -> OrderStatus:
        try:
            return self.ext_to_int_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order status, was {order_status}",  # pragma: no cover
            )

    def parse_binance_order_type(self, order_type: BinanceOrderType) -> OrderType:
        # Implement in child class
        raise NotImplementedError

    def parse_internal_order_type(self, order: Order) -> BinanceOrderType:
        # Implement in child class
        raise NotImplementedError

    def parse_binance_bar_agg(self, bar_agg: str) -> BarAggregation:
        try:
            return self.ext_to_int_bar_agg[bar_agg]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized Binance kline resolution, was {bar_agg}",
            )

    def parse_nautilus_bar_aggregation(self, bar_agg: BarAggregation) -> str:
        try:
            return self.int_to_ext_bar_agg[bar_agg]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                "unrecognized or non-supported Nautilus BarAggregation,",
                f"was {bar_aggregation_to_str(bar_agg)}",  # pragma: no cover
            )

    def parse_binance_kline_interval_to_bar_spec(
        self,
        kline_interval: BinanceKlineInterval,
    ) -> BarSpecification:
        step = kline_interval.value[:-1]
        binance_bar_agg = kline_interval.value[-1]
        return BarSpecification(
            step=int(step),
            aggregation=self.parse_binance_bar_agg(binance_bar_agg),
            price_type=PriceType.LAST,
        )

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        # Replace method in child class, if compatible
        raise NotImplementedError(  # pragma: no cover (design-time error)
            "Cannot parse binance trigger type (not implemented).",  # pragma: no cover
        )

    def parse_position_id_to_binance_futures_position_side(
        self,
        position_id: PositionId,
    ) -> BinanceFuturesPositionSide:
        if position_id.value.endswith("LONG"):  # Position Long
            return BinanceFuturesPositionSide.LONG
        elif position_id.value.endswith("SHORT"):  # Position Short
            return BinanceFuturesPositionSide.SHORT
        elif position_id.value.endswith("BOTH"):
            return BinanceFuturesPositionSide.BOTH
        else:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized position id, was {position_id}",  # pragma: no cover
            )
