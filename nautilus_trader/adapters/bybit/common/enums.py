# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce


def raise_error(error):
    raise error


@unique
class BybitPositionIdx(Enum):
    # one-way mode position
    ONE_WAY = 0
    # buy side of hedge-mode position
    BUY_HEDGE = 1
    # sell side of hedge-mode position
    SELL_HEDGE = 2


@unique
class BybitPositionSide(Enum):
    BUY = "Buy"
    SELL = "Sell"

    def parse_to_position_side(self) -> PositionSide:
        if self == BybitPositionSide.BUY:
            return PositionSide.LONG
        elif self == BybitPositionSide.SELL:
            return PositionSide.SHORT


@unique
class BybitKlineInterval(Enum):
    MINUTE_1 = "1"
    MINUTE_3 = "3"
    MINUTE_5 = "5"
    MINUTE_15 = "15"
    MINUTE_30 = "30"
    HOUR_1 = "60"
    HOUR_2 = "120"
    HOUR_4 = "240"
    HOUR_6 = "360"
    HOUR_12 = "720"
    DAY_1 = "D"
    WEEK_1 = "W"
    MONTH_1 = "M"


@unique
class BybitOrderStatus(Enum):
    CREATED = "Created"
    NEW = "New"
    REJECTED = "Rejected"
    PARTIALLY_FILLED = "PartiallyFilled"
    PARTIALLY_FILLED_CANCELED = "PartiallyFilledCanceled"
    FILLED = "Filled"
    CANCELED = "Cancelled"
    UNTRIGGERED = "Untriggered"
    TRIGGERED = "Triggered"
    DEACTIVATED = "Deactivated"
    ACTIVE = "Active"


@unique
class BybitOrderSide(Enum):
    BUY = "Buy"
    SELL = "Sell"


@unique
class BybitOrderType(Enum):
    MARKET = "Market"
    LIMIT = "Limit"
    UNKNOWN = "Unknown"


@unique
class BybitTriggerType(Enum):
    LAST_PRICE = "LastPrice"
    INDEX_PRICE = "IndexPrice"
    MARK_PRICE = "MarkPrice"


@unique
class BybitTimeInForce(Enum):
    GTC = "GTC"
    IOC = "IOC"
    FOK = "FOK"
    POST_ONLY = "PostOnly"


@unique
class BybitAccountType(Enum):
    UNIFIED = "UNIFIED"


@unique
class BybitInstrumentType(Enum):
    SPOT = "spot"
    LINEAR = "linear"
    INVERSE = "inverse"
    OPTION = "option"

    @property
    def is_spot_or_margin(self) -> bool:
        return self in [BybitInstrumentType.SPOT]

    @property
    def is_spot(self) -> bool:
        return self in [BybitInstrumentType.SPOT]


@unique
class BybitContractType(Enum):
    INVERSE_PERPETUAL = "InversePerpetual"
    LINEAR_PERPETUAL = "LinearPerpetual"
    LINEAR_FUTURE = "LinearFutures"
    INVERSE_FUTURE = "InverseFutures"


@unique
class BybitTransactionType(Enum):
    # Assets that transferred into Unified wallet
    TRANSFER_IN = "TRANSFER_IN"
    # Assets that transferred out of Unified wallet
    TRANSFER_OUT = "TRANSFER_OUT"
    TRADE = "TRADE"
    SETTLEMENT = "SETTLEMENT"
    DELIVERY = "DELIVERY"
    LIQUIDATION = "LIQUIDATION"
    AIRDROP = "AIRDRP"


def check_dict_keys(key, data):
    try:
        return data[key]
    except KeyError:
        raise RuntimeError(
            f"Unrecognized Bybit {key} not found in {data}",
        )


class BybitEnumParser:
    def __init__(self) -> None:
        self.bybit_to_nautilus_order_side = {
            BybitOrderSide.BUY: OrderSide.BUY,
            BybitOrderSide.SELL: OrderSide.SELL,
        }
        self.nautilus_to_bybit_order_side = {
            b: a for a, b in self.bybit_to_nautilus_order_side.items()
        }
        self.bybit_to_nautilus_order_type = {
            BybitOrderType.MARKET: OrderType.MARKET,
            BybitOrderType.LIMIT: OrderType.LIMIT,
        }
        self.nautilus_to_bybit_order_type = {
            b: a for a, b in self.bybit_to_nautilus_order_type.items()
        }

        # TODO check time in force mapping
        self.bybit_to_nautilus_time_in_force = {
            BybitTimeInForce.GTC: TimeInForce.GTC,
            BybitTimeInForce.IOC: TimeInForce.IOC,
            BybitTimeInForce.FOK: TimeInForce.FOK,
            BybitTimeInForce.POST_ONLY: TimeInForce.GTC,
        }
        self.nautilus_to_bybit_time_in_force = {
            TimeInForce.GTC: BybitTimeInForce.GTC,
            TimeInForce.IOC: BybitTimeInForce.IOC,
            TimeInForce.FOK: BybitTimeInForce.FOK,
        }

        self.bybit_to_nautilus_order_status = {
            BybitOrderStatus.CREATED: OrderStatus.SUBMITTED,
            BybitOrderStatus.NEW: OrderStatus.ACCEPTED,
            BybitOrderStatus.FILLED: OrderStatus.FILLED,
            BybitOrderStatus.CANCELED: OrderStatus.CANCELED,
            BybitOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
        }
        self.nautilus_to_bybit_order_status = {
            b: a for a, b in self.bybit_to_nautilus_order_status.items()
        }

        # klines
        self.minute_klines_interval = [1, 3, 5, 15, 30]
        self.hour_klines_interval = [1, 2, 4, 6, 12]
        self.aggregation_kline_mapping = {
            BarAggregation.MINUTE: lambda x: BybitKlineInterval(f"{x}"),
            BarAggregation.HOUR: lambda x: BybitKlineInterval(f"{x * 60}"),
            BarAggregation.DAY: lambda x: (
                BybitKlineInterval("D")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect day kline interval {x}"))
            ),
            BarAggregation.WEEK: lambda x: (
                BybitKlineInterval("W")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect week kline interval {x}"))
            ),
            BarAggregation.MONTH: lambda x: (
                BybitKlineInterval("M")
                if x == 1
                else raise_error(ValueError(f"Bybit incorrect month kline interval {x}"))
            ),
        }
        self.valid_order_types = {
            OrderType.MARKET,
            OrderType.LIMIT,
        }
        self.valid_time_in_force = {
            TimeInForce.GTC,
            TimeInForce.IOC,
            TimeInForce.FOK,
        }

    def parse_bybit_order_status(self, order_status: BybitOrderStatus) -> OrderStatus:
        return check_dict_keys(order_status, self.bybit_to_nautilus_order_status)

    def parse_nautilus_order_status(self, order_status: OrderStatus) -> BybitOrderStatus:
        return check_dict_keys(order_status, self.nautilus_to_bybit_order_status)

    def parse_bybit_time_in_force(self, time_in_force: BybitTimeInForce) -> TimeInForce:
        return check_dict_keys(time_in_force, self.bybit_to_nautilus_time_in_force)

    def parse_bybit_order_side(self, order_side: BybitOrderSide) -> OrderSide:
        return check_dict_keys(order_side, self.bybit_to_nautilus_order_side)

    def parse_nautilus_order_side(self, order_side: OrderSide) -> BybitOrderSide:
        return check_dict_keys(order_side, self.nautilus_to_bybit_order_side)

    def parse_bybit_order_type(self, order_type: BybitOrderType) -> OrderType:
        return check_dict_keys(order_type, self.bybit_to_nautilus_order_type)

    def parse_nautilus_order_type(self, order_type: OrderType) -> BybitOrderType:
        return check_dict_keys(order_type, self.nautilus_to_bybit_order_type)

    def parse_nautilus_time_in_force(self, time_in_force: TimeInForce) -> BybitTimeInForce:
        try:
            return self.nautilus_to_bybit_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_bybit_kline(self, bar_type: BarType) -> BybitKlineInterval:
        try:
            aggregation = bar_type.spec.aggregation
            interval = int(bar_type.spec.step)
            if aggregation in self.aggregation_kline_mapping:
                result = self.aggregation_kline_mapping[aggregation](interval)
                return result
            else:
                raise ValueError(
                    f"Bybit incorrect aggregation {aggregation}",  # pragma: no cover
                )
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit bar type, was {bar_type}",  # pragma: no cover
            )


@unique
class BybitEndpointType(Enum):
    NONE = "NONE"
    MARKET = "MARKET"
    ACCOUNT = "ACCOUNT"
    TRADE = "TRADE"
