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

from enum import Enum
from enum import unique

from nautilus_trader.core.nautilus_pyo3 import PositionSide
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TriggerType


# from nautilus_trader.model.data import BarType
# from nautilus_trader.model.enums import BarAggregation


@unique
class OKXWsBaseUrlType(Enum):
    PUBLIC = "public"
    PRIVATE = "private"
    BUSINESS = "business"


@unique
class OKXEndpointType(Enum):
    NONE = "NONE"  # /api/v5/?
    ASSET = "ASSET"  # /api/v5/asset
    MARKET = "MARKET"  # /api/v5/market
    ACCOUNT = "ACCOUNT"  # /api/v5/account
    PUBLIC = "PUBLIC"  # /api/v5/public
    RUBIK_STAT = "RUBIK_STAT"  # /api/v5/rubik/stat
    TRADE = "TRADE"  # /api/v5/trade
    USERS = "USERS"  # /api/v5/users
    BROKER = "BROKER"  # /api/v5/broker
    RFQ = "RFQ"  # /api/v5/rfq
    TRADING_BOT = "TRADING_BOT"  # /api/v5/tradingBot
    FINANCE = "FINANCE"  # /api/v5/finance
    SYSTEM_STATUS = "SYSTEM_STATUS"  # /api/v5/system/status
    COPY_TRADING = "COPY_TRADING"  # /api/v5/copytrading
    SPREAD_TRADING = "SPREAD_TRADING"  # /api/v5/sprd


@unique
class OKXInstrumentType(Enum):
    ANY = "ANY"
    SPOT = "SPOT"
    MARGIN = "MARGIN"
    SWAP = "SWAP"
    FUTURES = "FUTURES"
    OPTION = "OPTION"

    @property
    def is_spot(self) -> bool:
        return self == OKXInstrumentType.SPOT

    @property
    def is_margin(self) -> bool:
        return self == OKXInstrumentType.MARGIN

    @property
    def is_swap(self) -> bool:
        return self == OKXInstrumentType.SWAP

    @property
    def is_futures(self) -> bool:
        return self == OKXInstrumentType.FUTURES

    @property
    def is_option(self) -> bool:
        return self == OKXInstrumentType.OPTION


@unique
class OKXInstrumentStatus(Enum):
    LIVE = "live"
    SUSPEND = "suspend"
    PREOPEN = "preopen"
    TEST = "test"


@unique
class OKXContractType(Enum):
    NONE = ""
    LINEAR = "linear"
    INVERSE = "inverse"

    @property
    def is_linear(self) -> bool:
        return self == OKXContractType.LINEAR

    @property
    def is_inverse(self) -> bool:
        return self == OKXContractType.INVERSE

    @staticmethod
    def find(ctType: str) -> "OKXContractType":
        match ctType:
            case "":
                return OKXContractType.NONE
            case "linear":
                return OKXContractType.LINEAR
            case "inverse":
                return OKXContractType.INVERSE
            case _:
                raise ValueError(f"Could not find matching OKXContractType for `ctType` {ctType!r}")


@unique
class OKXBarSize(Enum):
    SECOND_1 = "1s"
    MINUTE_1 = "1m"
    MINUTE_3 = "3m"
    MINUTE_5 = "5m"
    MINUTE_15 = "15m"
    MINUTE_30 = "30m"
    HOUR_1 = "1H"
    HOUR_2 = "2H"
    HOUR_4 = "4H"
    HOUR_6 = "6H"
    HOUR_12 = "12H"
    DAY_1 = "1D"
    DAY_2 = "2D"
    DAY_3 = "3D"
    DAY_5 = "5D"
    WEEK_1 = "1W"
    MONTH_1 = "1M"
    MONTH_3 = "3M"


@unique
class OKXTradeMode(Enum):
    ISOLATED = "isolated"  # Margin account
    CROSS = "cross"  # Margin account
    CASH = "cash"  # Cash account
    SPOT_ISOLATED = "spot_isolated"  # only applicable to SPOT lead trading


@unique
class OKXAccountMode(Enum):
    SPOT = "Spot mode"
    SPOT_AND_FUTURES = "Spot and futures mode"
    MULTI_CURRENCY_MARGIN_MODE = "Multi-currency margin mode"
    PORTFOLIO_MARGIN_MODE = "Portfolio margin mode"


@unique
class OKXMarginMode(Enum):
    ISOLATED = "isolated"
    CROSS = "cross"
    NONE = ""


@unique
class OKXOrderSide(Enum):
    BUY = "buy"
    SELL = "sell"


@unique
class OKXExecutionType(Enum):
    NONE = ""
    TAKER = "T"
    MAKER = "M"

    def parse_to_liquidity_side(self) -> LiquiditySide:
        assert self is not OKXExecutionType.NONE, f"Cannot parse LiquiditySide from {self}"

        return LiquiditySide.MAKER if self is OKXExecutionType.MAKER else LiquiditySide.TAKER


@unique
class OKXOrderStatus(Enum):  # "state"
    CANCELED = "canceled"
    LIVE = "live"
    PARTIALLY_FILLED = "partially_filled"
    FILLED = "filled"
    MMP_CANCELED = "mmp_canceled"


@unique
class OKXPositionSide(Enum):
    NET = "net"
    LONG = "long"
    SHORT = "short"
    NONE = ""

    def parse_to_position_side(self, pos_qty: str) -> PositionSide:
        if pos_qty == "" or float(pos_qty) == 0:
            return PositionSide.FLAT

        if self == OKXPositionSide.LONG:
            return PositionSide.LONG
        elif self == OKXPositionSide.SHORT:
            return PositionSide.SHORT
        elif self == OKXPositionSide.NET:
            return (
                PositionSide.LONG
                if float(pos_qty) > 0
                else PositionSide.SHORT if float(pos_qty) < 0 else PositionSide.FLAT
            )
        raise RuntimeError(f"invalid position side and/or `pos_qty`, was {self} and {pos_qty=}")


@unique
class OKXOrderType(Enum):
    MARKET = "market"
    LIMIT = "limit"
    POST_ONLY = "post_only"  # limit only, requires "px" to be provided
    FOK = "fok"  # market order if "px" is not provided, otherwise limit order
    IOC = "ioc"  # market order if "px" is not provided, otherwise limit order
    OPTIMAL_LIMIT_IOC = "optimal_limit_ioc"  # Market order with immediate-or-cancel order
    MMP = "mmp"  # Market Maker Protection (only applicable to Option in Portfolio Margin mode)
    MMP_AND_POST_ONLY = "mmp_and_post_only"  # Market Maker Protection and Post-only order(only applicable to Option in Portfolio Margin mode)


@unique
class OKXSelfTradePreventionMode(Enum):
    NONE = ""
    CANCEL_MAKER = "cancel_maker"
    CANCEL_TAKER = "cancel_taker"
    CANCEL_BOTH = "cancel_both"  # Cancel both does not support FOK


@unique
class OKXTakeProfitKind(Enum):
    NONE = ""
    CONDITION = "condition"
    LIMIT = "limit"


@unique
class OKXTriggerType(Enum):
    NONE = ""
    LAST = "last"
    INDEX = "index"
    MARK = "mark"


@unique
class OKXAlgoOrderType(Enum):
    CONDITIONAL = "conditional"  # one-way stop
    OCO = "oco"
    TRIGGER = "trigger"  # trigger order
    MOVE_ORDER_STOP = "move_order_stop"  # trailing stop
    ICEBERG = "iceberg"
    TWAP = "twap"


@unique
class OKXAlgoOrderStatus(Enum):  # "state"
    LIVE = "live"  # to be effective
    PAUSE = "pause"
    PARTIALLY_EFFECTIVE = "partially_effective"
    EFFECTIVE = "effective"
    CANCELED = "canceled"
    ORDER_FAILED = "order_failed"
    PARTIALLY_FAILED = "partially_failed"  # TODO: typo in docs? should it be "partially_filled"?


@unique
class OKXTransactionType(Enum):
    BUY = "1"
    SELL = "2"
    OPEN_LONG = "3"
    OPEN_SHORT = "4"
    CLOSE_LONG = "5"
    CLOSE_SHORT = "6"
    PARTIAL_LIQUIDATION_CLOSE_LONG = "100"
    PARTIAL_LIQUIDATION_CLOSE_SHORT = "101"
    PARTIAL_LIQUIDATION_BUY = "102"
    PARTIAL_LIQUIDATION_SELL = "103"
    LIQUIDATION_LONG = "104"
    LIQUIDATION_SHORT = "105"
    LIQUIDATION_BUY = "106"
    LIQUIDATION_SELL = "107"
    LIQUIDATION_TRANSFER_IN = "110"
    LIQUIDATION_TRANSFER_OUT = "111"
    SYSTEM_TOKEN_CONVERSION_TRANSFER_IN = "118"
    SYSTEM_TOKEN_CONVERSION_TRANSFER_OUT = "119"
    ADL_CLOSE_LONG = "125"
    ADL_CLOSE_SHORT = "126"
    ADL_BUY = "127"
    ADL_SELL = "128"
    AUTO_BORROW_OF_QUICK_MARGIN = "212"
    AUTO_REPAY_OF_QUICK_MARGIN = "213"
    BLOCK_TRADE_BUY = "204"
    BLOCK_TRADE_SELL = "205"
    BLOCK_TRADE_OPEN_LONG = "206"
    BLOCK_TRADE_OPEN_SHORT = "207"
    BLOCK_TRADE_CLOSE_OPEN = "208"
    BLOCK_TRADE_CLOSE_SHORT = "209"
    SPREAD_TRADING_BUY = "270"
    SPREAD_TRADING_SELL = "271"
    SPREAD_TRADING_OPEN_LONG = "272"
    SPREAD_TRADING_OPEN_SHORT = "273"
    SPREAD_TRADING_CLOSE_LONG = "274"
    SPREAD_TRADING_CLOSE_SHORT = "275"


def check_dict_keys(key, data):
    try:
        return data[key]
    except KeyError:
        raise RuntimeError(
            f"Unrecognized OKX {key} not found in {data}",
        )


class OKXEnumParser:
    def __init__(self) -> None:
        # OrderSide
        self.okx_to_nautilus_order_side = {
            OKXOrderSide.BUY: OrderSide.BUY,
            OKXOrderSide.SELL: OrderSide.SELL,
        }
        self.nautilus_to_okx_order_side = {b: a for a, b in self.okx_to_nautilus_order_side.items()}

        # fmt: off
        self.okx_to_nautilus_order_status = {
            (OrderType.MARKET, OKXOrderStatus.LIVE): OrderStatus.ACCEPTED,
            (OrderType.MARKET, OKXOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.MARKET, OKXOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.MARKET, OKXOrderStatus.FILLED): OrderStatus.FILLED,

            (OrderType.LIMIT, OKXOrderStatus.LIVE): OrderStatus.ACCEPTED,
            (OrderType.LIMIT, OKXOrderStatus.CANCELED): OrderStatus.CANCELED,
            (OrderType.LIMIT, OKXOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            (OrderType.LIMIT, OKXOrderStatus.FILLED): OrderStatus.FILLED,

            # TODO: map advanced order types to okx's algo trading api

            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.MARKET_IF_TOUCHED, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,

            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.LIMIT_IF_TOUCHED, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,

            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.STOP_MARKET, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,

            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.STOP_LIMIT, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,

            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.TRAILING_STOP_MARKET, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,

            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.CREATED): OrderStatus.SUBMITTED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.NEW): OrderStatus.ACCEPTED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.REJECTED): OrderStatus.REJECTED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.CANCELED): OrderStatus.CANCELED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.UNTRIGGERED): OrderStatus.ACCEPTED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.TRIGGERED): OrderStatus.TRIGGERED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.DEACTIVATED): OrderStatus.CANCELED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.PARTIALLY_FILLED): OrderStatus.PARTIALLY_FILLED,
            # (OrderType.TRAILING_STOP_LIMIT, OKXAlgoOrderStatus.FILLED): OrderStatus.FILLED,
        }
        # fmt: on

        self.okx_to_nautilus_trigger_type = {
            OKXTriggerType.NONE: TriggerType.NO_TRIGGER,
            OKXTriggerType.LAST: TriggerType.LAST_PRICE,
            OKXTriggerType.MARK: TriggerType.MARK_PRICE,
            OKXTriggerType.INDEX: TriggerType.INDEX_PRICE,
        }
        self.nautilus_to_okx_trigger_type = {
            b: a for a, b in self.okx_to_nautilus_trigger_type.items()
        }
        self.nautilus_to_okx_trigger_type[TriggerType.DEFAULT] = OKXTriggerType.LAST

        # # klines
        # self.minute_klines_interval = [1, 3, 5, 15, 30]
        # self.hour_klines_interval = [1, 2, 4, 6, 12]
        # self.aggregation_kline_mapping = {
        #     BarAggregation.MINUTE: lambda x: OKXKlineInterval(f"{x}"),
        #     BarAggregation.HOUR: lambda x: OKXKlineInterval(f"{x * 60}"),
        #     BarAggregation.DAY: lambda x: (
        #         OKXKlineInterval("D")
        #         if x == 1
        #         else raise_error(ValueError(f"OKX incorrect day kline interval {x}"))
        #     ),
        #     BarAggregation.WEEK: lambda x: (
        #         OKXKlineInterval("W")
        #         if x == 1
        #         else raise_error(ValueError(f"OKX incorrect week kline interval {x}"))
        #     ),
        #     BarAggregation.MONTH: lambda x: (
        #         OKXKlineInterval("M")
        #         if x == 1
        #         else raise_error(ValueError(f"OKX incorrect month kline interval {x}"))
        #     ),
        # }
        self.valid_time_in_force = {
            TimeInForce.GTC,
            TimeInForce.IOC,
            TimeInForce.FOK,
        }

    def parse_okx_order_status(
        self,
        order_type: OrderType,
        order_status: OKXOrderStatus,
    ) -> OrderStatus:
        return check_dict_keys((order_type, order_status), self.okx_to_nautilus_order_status)

    def parse_okx_order_side(self, order_side: OKXOrderSide) -> OrderSide:
        return check_dict_keys(order_side, self.okx_to_nautilus_order_side)

    def parse_nautilus_order_side(self, order_side: OrderSide) -> OKXOrderSide:
        return check_dict_keys(order_side, self.nautilus_to_okx_order_side)

    def parse_nautilus_trigger_type(self, trigger_type: TriggerType) -> OKXTriggerType:
        return check_dict_keys(trigger_type, self.nautilus_to_okx_trigger_type)

    def parse_okx_trigger_type(self, trigger_type: OKXTriggerType) -> TriggerType:
        return check_dict_keys(trigger_type, self.okx_to_nautilus_trigger_type)

    def parse_okx_order_type(self, ordType: OKXOrderType) -> OrderType:
        # TODO add parameters in future to enable parsing of all other nautilus OrderType's
        match ordType:
            case OKXOrderType.MARKET:
                return OrderType.MARKET
            case OKXOrderType.LIMIT:
                return OrderType.LIMIT
            case OKXOrderType.IOC:
                return OrderType.LIMIT
            case OKXOrderType.FOK:
                return OrderType.LIMIT
            case OKXOrderType.POST_ONLY:
                return OrderType.LIMIT
            case _:
                raise NotImplementedError(f"Cannot parse OrderType from OKX order type {ordType}")

    def parse_okx_time_in_force(self, ordType: OKXOrderType) -> TimeInForce:
        match ordType:
            case OKXOrderType.MARKET:
                return TimeInForce.GTC
            case OKXOrderType.LIMIT:
                return TimeInForce.GTC
            case OKXOrderType.POST_ONLY:
                return TimeInForce.GTC
            case OKXOrderType.FOK:
                return TimeInForce.FOK
            case OKXOrderType.IOC:
                return TimeInForce.IOC
            case _:
                raise NotImplementedError(f"Cannot parse TimeInForce from OKX order type {ordType}")

    # def parse_okx_kline(self, bar_type: BarType) -> OKXKlineInterval:
    #     try:
    #         aggregation = bar_type.spec.aggregation
    #         interval = int(bar_type.spec.step)
    #         if aggregation in self.aggregation_kline_mapping:
    #             result = self.aggregation_kline_mapping[aggregation](interval)
    #             return result
    #         else:
    #             raise ValueError(
    #                 f"OKX incorrect aggregation {aggregation}",  # pragma: no cover
    #             )
    #     except KeyError:
    #         raise RuntimeError(
    #             f"unrecognized OKX bar type, was {bar_type}",  # pragma: no cover
    #         )
