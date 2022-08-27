# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from decimal import Decimal

from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesOrderType
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesTimeInForce
from nautilus_trader.adapters.binance.futures.enums import BinanceFuturesWorkingType
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesAccountTrade
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesOrder
from nautilus_trader.adapters.binance.futures.schemas.account import BinanceFuturesPositionRisk
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import Order


def binance_order_type(order: Order) -> BinanceFuturesOrderType:
    if order.type == OrderType.MARKET:
        return BinanceFuturesOrderType.MARKET
    elif order.type == OrderType.LIMIT:
        return BinanceFuturesOrderType.LIMIT
    elif order.type == OrderType.STOP_MARKET:
        return BinanceFuturesOrderType.STOP_MARKET
    elif order.type == OrderType.STOP_LIMIT:
        return BinanceFuturesOrderType.STOP
    elif order.type == OrderType.MARKET_IF_TOUCHED:
        return BinanceFuturesOrderType.TAKE_PROFIT_MARKET
    elif order.type == OrderType.LIMIT_IF_TOUCHED:
        return BinanceFuturesOrderType.TAKE_PROFIT
    elif order.type == OrderType.TRAILING_STOP_MARKET:
        return BinanceFuturesOrderType.TRAILING_STOP_MARKET
    else:  # pragma: no cover (design-time error)
        raise RuntimeError("invalid order type")


def parse_order_type(order_type: BinanceFuturesOrderType) -> OrderType:
    if order_type == BinanceFuturesOrderType.STOP:
        return OrderType.STOP_LIMIT
    elif order_type == BinanceFuturesOrderType.STOP_MARKET:
        return OrderType.STOP_MARKET
    elif order_type == BinanceFuturesOrderType.TAKE_PROFIT:
        return OrderType.LIMIT_IF_TOUCHED
    elif order_type == BinanceFuturesOrderType.TAKE_PROFIT_MARKET:
        return OrderType.MARKET_IF_TOUCHED
    else:
        return OrderType[order_type.value]


def parse_order_status(status: BinanceOrderStatus) -> OrderStatus:
    if status == BinanceOrderStatus.NEW:
        return OrderStatus.ACCEPTED
    elif status == BinanceOrderStatus.CANCELED:
        return OrderStatus.CANCELED
    elif status == BinanceOrderStatus.PARTIALLY_FILLED:
        return OrderStatus.PARTIALLY_FILLED
    elif status == BinanceOrderStatus.FILLED:
        return OrderStatus.FILLED
    elif status == BinanceOrderStatus.NEW_ADL:
        return OrderStatus.FILLED
    elif status == BinanceOrderStatus.NEW_INSURANCE:
        return OrderStatus.FILLED
    elif status == BinanceOrderStatus.EXPIRED:
        return OrderStatus.EXPIRED
    else:  # pragma: no cover (design-time error)
        raise RuntimeError(f"unrecognized order status, was {status}")


def parse_time_in_force(time_in_force: BinanceFuturesTimeInForce) -> TimeInForce:
    if time_in_force == BinanceFuturesTimeInForce.GTX:
        return TimeInForce.GTC
    else:
        return TimeInForce[time_in_force.value]


def parse_trigger_type(working_type: BinanceFuturesWorkingType) -> TriggerType:
    if working_type == BinanceFuturesWorkingType.CONTRACT_PRICE:
        return TriggerType.LAST
    elif working_type == BinanceFuturesWorkingType.MARK_PRICE:
        return TriggerType.MARK
    else:  # pragma: no cover (design-time error)
        return TriggerType.NONE


def parse_order_report_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: BinanceFuturesOrder,
    report_id: UUID4,
    ts_init: int,
) -> OrderStatusReport:
    price = Decimal(data.price)
    trigger_price = Decimal(data.stopPrice)
    avg_px = Decimal(data.avgPrice)
    time_in_force = BinanceFuturesTimeInForce(data.timeInForce.upper())
    return OrderStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        client_order_id=ClientOrderId(data.clientOrderId) if data.clientOrderId != "" else None,
        venue_order_id=VenueOrderId(str(data.orderId)),
        order_side=OrderSide[data.side.upper()],
        order_type=parse_order_type(data.type),
        time_in_force=parse_time_in_force(time_in_force),
        order_status=parse_order_status(data.status),
        price=Price.from_str(data.price) if price is not None else None,
        quantity=Quantity.from_str(data.origQty),
        filled_qty=Quantity.from_str(data.executedQty),
        avg_px=avg_px if avg_px > 0 else None,
        post_only=time_in_force == BinanceFuturesTimeInForce.GTX,
        reduce_only=data.reduceOnly,
        report_id=report_id,
        ts_accepted=millis_to_nanos(data.time),
        ts_last=millis_to_nanos(data.updateTime),
        ts_init=ts_init,
        trigger_price=Price.from_str(str(trigger_price)) if trigger_price > 0 else None,
        trigger_type=parse_trigger_type(data.workingType),
        trailing_offset=Decimal(data.priceRate) * 100 if data.priceRate is not None else None,
        trailing_offset_type=TrailingOffsetType.BASIS_POINTS
        if data.priceRate is not None
        else TrailingOffsetType.NONE,
    )


def parse_trade_report_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: BinanceFuturesAccountTrade,
    report_id: UUID4,
    ts_init: int,
) -> TradeReport:
    return TradeReport(
        account_id=account_id,
        instrument_id=instrument_id,
        venue_order_id=VenueOrderId(str(data.orderId)),
        venue_position_id=PositionId(f"{instrument_id}-{data.positionSide.value}"),
        trade_id=TradeId(str(data.id)),
        order_side=OrderSide[data.side.value],
        last_qty=Quantity.from_str(data.qty),
        last_px=Price.from_str(data.price),
        commission=Money(data.commission, Currency.from_str(data.commissionAsset)),
        liquidity_side=LiquiditySide.MAKER if data.maker else LiquiditySide.TAKER,
        report_id=report_id,
        ts_event=millis_to_nanos(data.time),
        ts_init=ts_init,
    )


def parse_position_report_http(
    account_id: AccountId,
    instrument_id: InstrumentId,
    data: BinanceFuturesPositionRisk,
    report_id: UUID4,
    ts_init: int,
) -> PositionStatusReport:
    net_size = Decimal(data.positionAmt)

    if net_size > 0:
        position_side = PositionSide.LONG
    elif net_size < 0:
        position_side = PositionSide.SHORT
    else:
        position_side = PositionSide.FLAT

    return PositionStatusReport(
        account_id=account_id,
        instrument_id=instrument_id,
        position_side=position_side,
        quantity=Quantity.from_str(str(abs(net_size))),
        report_id=report_id,
        ts_last=ts_init,
        ts_init=ts_init,
    )
