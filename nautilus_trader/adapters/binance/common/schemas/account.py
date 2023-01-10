# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Optional

import msgspec

from nautilus_trader.adapters.binance.common.enums import BinanceOrderSide
from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.parsing.execution import BinanceExecutionParser
from nautilus_trader.adapters.binance.common.schemas.symbol import BinanceSymbol
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import OrderListId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# HTTP responses
################################################################################


class BinanceOrder(msgspec.Struct, frozen=True):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/order`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/order`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/order`
    """

    symbol: BinanceSymbol
    orderId: int
    clientOrderId: str
    price: str
    origQty: str
    executedQty: str
    status: BinanceOrderStatus
    timeInForce: BinanceTimeInForce
    type: BinanceOrderType
    side: BinanceOrderSide
    stopPrice: str  # please ignore when order type is TRAILING_STOP_MARKET
    time: int
    updateTime: int

    orderListId: Optional[int] = None  # SPOT/MARGIN only. Unless OCO, the value will always be -1
    cumulativeQuoteQty: Optional[str] = None  # SPOT/MARGIN only, cumulative quote qty
    icebergQty: Optional[str] = None  # SPOT/MARGIN only
    isWorking: Optional[bool] = None  # SPOT/MARGIN only
    workingTime: Optional[int] = None  # SPOT/MARGIN only
    origQuoteOrderQty: Optional[str] = None  # SPOT/MARGIN only
    selfTradePreventionMode: Optional[str] = None  # SPOT/MARGIN only

    avgPrice: Optional[str] = None  # FUTURES only
    origType: Optional[BinanceOrderType] = None  # FUTURES only
    reduceOnly: Optional[bool] = None  # FUTURES only
    positionSide: Optional[str] = None  # FUTURES only
    closePosition: Optional[bool] = None  # FUTURES only, if Close-All
    activatePrice: Optional[
        str
    ] = None  # FUTURES only, activation price, only return with TRAILING_STOP_MARKET order
    priceRate: Optional[
        str
    ] = None  # FUTURES only, callback rate, only return with TRAILING_STOP_MARKET order
    workingType: Optional[str] = None  # FUTURES only
    priceProtect: Optional[bool] = None  # FUTURES only, if conditional order trigger is protected

    cumQuote: Optional[str] = None  # USD-M FUTURES only

    cumBase: Optional[str] = None  # COIN-M FUTURES only
    pair: Optional[str] = None  # COIN-M FUTURES only

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        exec_parser: BinanceExecutionParser,
        ts_init: int,
    ) -> OrderStatusReport:
        client_order_id = ClientOrderId(self.clientOrderId) if self.clientOrderId != "" else None
        order_list_id = OrderListId(str(self.orderListId)) if self.orderListId is not None else None
        contingency_type = (
            ContingencyType.OCO
            if self.orderListId is not None and self.orderListId != -1
            else ContingencyType.NO_CONTINGENCY
        )

        trigger_price = Decimal(self.stopPrice)
        trigger_type = TriggerType.NO_TRIGGER
        if self.workingType is not None:
            trigger_type = exec_parser.parse_binance_trigger_type(self.workingType)
        elif trigger_price > 0:
            trigger_type = TriggerType.LAST_TRADE if trigger_price > 0 else TriggerType.NO_TRIGGER

        trailing_offset = None
        trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET
        if self.priceRate is not None:
            trailing_offset = Decimal(self.priceRate)
            trailing_offset_type = TrailingOffsetType.BASIS_POINTS

        avg_px = Decimal(self.avgPrice) if self.avgPrice is not None else None
        post_only = (
            self.type == BinanceOrderType.LIMIT_MAKER or self.timeInForce == BinanceTimeInForce.GTX
        )
        reduce_only = self.reduceOnly if self.reduceOnly is not None else False

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_list_id=order_list_id,
            venue_order_id=VenueOrderId(str(self.orderId)),
            order_side=OrderSide[self.side],
            order_type=exec_parser.parse_binance_order_type(self.type),
            contingency_type=contingency_type,
            time_in_force=exec_parser.parse_binance_time_in_force(self.timeInForce),
            order_status=exec_parser.parse_binance_order_status(self.status),
            price=Price.from_str(str(Decimal(self.price))),
            trigger_price=Price.from_str(str(trigger_price)),
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            quantity=Quantity.from_str(self.origQty),
            filled_qty=Quantity.from_str(self.executedQty),
            avg_px=avg_px,
            post_only=post_only,
            reduce_only=reduce_only,
            ts_accepted=millis_to_nanos(self.time),
            ts_last=millis_to_nanos(self.updateTime),
            report_id=report_id,
            ts_init=ts_init,
        )


class BinanceUserTrade(msgspec.Struct):
    """
    HTTP response from `Binance Spot/Margin`
        `GET /api/v3/myTrades`
    HTTP response from `Binance USD-M Futures`
        `GET /fapi/v1/userTrades`
    HTTP response from `Binance COIN-M Futures`
        `GET /dapi/v1/userTrades`
    """

    symbol: BinanceSymbol
    id: int
    orderId: int
    commission: str
    commissionAsset: str
    price: str
    qty: str
    time: int

    quoteQty: Optional[str] = None  # SPOT/MARGIN & USD-M FUTURES only

    orderListId: Optional[int] = None  # SPOT/MARGIN only. Unless OCO, the value will always be -1
    isBuyer: Optional[bool] = None  # SPOT/MARGIN only
    isMaker: Optional[bool] = None  # SPOT/MARGIN only
    isBestMatch: Optional[bool] = None  # SPOT/MARGIN only

    buyer: Optional[bool] = None  # FUTURES only
    maker: Optional[bool] = None  # FUTURES only
    realizedPnl: Optional[str] = None  # FUTURES only
    side: Optional[BinanceOrderSide] = None  # FUTURES only
    positionSide: Optional[str] = None  # FUTURES only

    baseQty: Optional[str] = None  # COIN-M FUTURES only
    pair: Optional[str] = None  # COIN-M FUTURES only

    def parse_to_trade_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        ts_init: int,
    ) -> TradeReport:
        venue_position_id = None
        if self.positionSide is not None:
            venue_position_id = PositionId(f"{instrument_id}-{self.positionSide}")

        order_side = OrderSide.BUY if self.isBuyer or self.buyer else OrderSide.SELL
        liquidity_side = LiquiditySide.MAKER if self.isMaker or self.maker else LiquiditySide.TAKER

        return TradeReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(str(self.orderId)),
            venue_position_id=venue_position_id,
            trade_id=TradeId(str(self.id)),
            order_side=order_side,
            last_qty=Quantity.from_str(self.qty),
            last_px=Price.from_str(self.price),
            liquidity_side=liquidity_side,
            ts_event=millis_to_nanos(self.time),
            commission=Money(self.commission, Currency.from_str(self.commissionAsset)),
            report_id=report_id,
            ts_init=ts_init,
        )
