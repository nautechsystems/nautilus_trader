from enum import Enum
from enum import unique

from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import BarAggregation
def raise_error(error):
    raise error

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
    SPOT = "SPOT"
    LINEAR = "LINEAR"
    INVERSE = "INVERSE"
    OPTION = "OPTION"

    @property
    def is_spot_or_margin(self) -> bool:
        return self in [BybitInstrumentType.SPOT]

    @property
    def is_spot(self) -> bool:
        return self in [BybitInstrumentType.SPOT]


class BybitEnumParser:
    def __init__(self) -> None:
        self.ext_to_int_order_side = {
            BybitOrderSide.BUY: OrderSide.BUY,
            BybitOrderSide.SELL: OrderSide.SELL,
        }
        self.ext_to_int_order_type = {
            BybitOrderType.MARKET: OrderType.MARKET,
            BybitOrderType.LIMIT: OrderType.LIMIT,
        }
        # TODO check time in force mapping
        self.ext_to_int_time_in_force = {
            BybitTimeInForce.GTC: TimeInForce.GTC,
            BybitTimeInForce.IOC: TimeInForce.IOC,
            BybitTimeInForce.FOK: TimeInForce.FOK,
            BybitTimeInForce.POST_ONLY: TimeInForce.GTC,
        }
        self.ext_to_int_order_status = {
            BybitOrderStatus.CREATED: OrderStatus.ACCEPTED,
            BybitOrderStatus.NEW: OrderStatus.INITIALIZED,
            BybitOrderStatus.REJECTED: OrderStatus.REJECTED,
            BybitOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            BybitOrderStatus.PARTIALLY_FILLED_CANCELED: OrderStatus.PARTIALLY_FILLED,
            BybitOrderStatus.FILLED: OrderStatus.FILLED,
            BybitOrderStatus.CANCELED: OrderStatus.CANCELED,
            BybitOrderStatus.UNTRIGGERED: OrderStatus.RELEASED,
            BybitOrderStatus.TRIGGERED: OrderStatus.TRIGGERED,
            BybitOrderStatus.DEACTIVATED: OrderStatus.CANCELED,
            BybitOrderStatus.ACTIVE: OrderStatus.ACCEPTED,
        }
        # klines
        self.minute_klines_interval = [1,3,5,15,30]
        self.hour_klines_interval = [1,2,4,6,12]
        self.aggregation_kline_mapping = {
            BarAggregation.MINUTE: lambda x: BybitKlineInterval(f"{x}"),
            BarAggregation.HOUR: lambda x: BybitKlineInterval(f"{x * 60}"),
            BarAggregation.DAY: lambda x: BybitKlineInterval("D") if x == 1 else raise_error(ValueError(f"Bybit incorrect day kline interval {x}")),
            BarAggregation.WEEK: lambda x: BybitKlineInterval("W") if x == 1 else raise_error(ValueError(f"Bybit incorrect week kline interval {x}")),
            BarAggregation.MONTH: lambda x: BybitKlineInterval("M") if x == 1 else raise_error(ValueError(f"Bybit incorrect month kline interval {x}"))
        }

    def parse_bybit_order_status(self, order_status: BybitOrderStatus) -> OrderStatus:
        try:
            return self.ext_to_int_order_status[order_status]
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit order status, was {order_status}",  # pragma: no cover
            )

    def parse_bybit_time_in_force(self, time_in_force: BybitTimeInForce) -> TimeInForce:
        try:
            return self.ext_to_int_time_in_force[time_in_force]
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit time in force, was {time_in_force}",  # pragma: no cover
            )

    def parse_bybit_order_side(self, order_side: BybitOrderSide) -> OrderSide:
        try:
            return self.ext_to_int_order_side[order_side]
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit order side, was {order_side}",  # pragma: no cover
            )

    def parse_bybit_order_type(self, order_type: BybitOrderType) -> OrderType:
        try:
            return self.ext_to_int_order_type[order_type]
        except KeyError:
            raise RuntimeError(
                f"unrecognized Bybit order type, was {order_type}",  # pragma: no cover
            )



    def parse_bybit_kline(self, bar_type: BarType)-> BybitKlineInterval:
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
