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

from nautilus_trader.adapters.binance.common.enums import BinanceOrderStatus
from nautilus_trader.adapters.binance.common.enums import BinanceOrderType
from nautilus_trader.adapters.binance.common.enums import BinanceTimeInForce
from nautilus_trader.adapters.binance.common.schemas.schemas import BinanceOrder
from nautilus_trader.adapters.binance.common.schemas.schemas import BinanceUserTrade
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import TradeReport
from nautilus_trader.model.currency import Currency
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import TimeInForce
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


class BinanceExecutionParser:
    """
    Provides common parsing methods for execution on the 'Binance' exchange.

    Warnings:
    --------
    This class should not be used directly, but through a concrete subclass.
    """

    def __init__(self) -> None:
        # Construct dictionary hashmaps
        self.ext_status_to_int_status = {
            BinanceOrderStatus.NEW: OrderStatus.ACCEPTED,
            BinanceOrderStatus.CANCELED: OrderStatus.CANCELED,
            BinanceOrderStatus.PARTIALLY_FILLED: OrderStatus.PARTIALLY_FILLED,
            BinanceOrderStatus.FILLED: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_ADL: OrderStatus.FILLED,
            BinanceOrderStatus.NEW_INSURANCE: OrderStatus.FILLED,
            BinanceOrderStatus.EXPIRED: OrderStatus.EXPIRED,
        }

        # NOTE: There was some asymmetry in the original `parse_order_type` functions for SPOT & FUTURES
        # need to check that the below is absolutely correct..
        self.ext_order_type_to_int_order_type = {
            BinanceOrderType.STOP: OrderType.STOP_LIMIT,
            BinanceOrderType.STOP_LOSS: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_MARKET: OrderType.STOP_MARKET,
            BinanceOrderType.STOP_LOSS_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.TAKE_PROFIT: OrderType.LIMIT_IF_TOUCHED,
            BinanceOrderType.TAKE_PROFIT_LIMIT: OrderType.STOP_LIMIT,
            BinanceOrderType.TAKE_PROFIT_MARKET: OrderType.MARKET_IF_TOUCHED,
            BinanceOrderType.LIMIT: OrderType.LIMIT,
            BinanceOrderType.LIMIT_MAKER: OrderType.LIMIT,
        }

        # Build symmetrical reverse dictionary hashmaps
        self._build_int_to_ext_dicts()

    def _build_int_to_ext_dicts(self):
        self.int_status_to_ext_status = dict(
            map(
                reversed,
                self.ext_status_to_int_status.items(),
            ),
        )
        self.int_order_type_to_ext_order_type = dict(
            map(
                reversed,
                self.ext_order_type_to_int_order_type.items(),
            ),
        )

    def parse_binance_time_in_force(self, time_in_force: BinanceTimeInForce) -> TimeInForce:
        if time_in_force == BinanceTimeInForce.GTX:
            return TimeInForce.GTC
        else:
            return TimeInForce[time_in_force.value]

    def parse_binance_order_status(self, order_status: BinanceOrderStatus) -> OrderStatus:
        try:
            return self.ext_status_to_int_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order status, was {order_status}",  # pragma: no cover
            )

    def parse_internal_order_status(self, order_status: OrderStatus) -> BinanceOrderStatus:
        try:
            return self.int_status_to_ext_status[order_status]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized internal order status, was {order_status}",  # pragma: no cover
            )

    def parse_binance_order_type(self, order_type: BinanceOrderType) -> OrderType:
        try:
            return self.ext_order_type_to_int_order_type[order_type]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized binance order type, was {order_type}",  # pragma: no cover
            )

    def parse_internal_order_type(self, order_type: OrderType) -> BinanceOrderType:
        try:
            return self.int_order_type_to_ext_order_type[order_type]
        except KeyError:
            raise RuntimeError(  # pragma: no cover (design-time error)
                f"unrecognized internal order type, was {order_type}",  # pragma: no cover
            )

    def parse_binance_trigger_type(self, trigger_type: str) -> TriggerType:
        # Replace method in child class, if compatible
        raise RuntimeError(  # pragma: no cover (design-time error)
            "Cannot parse binance trigger type (not implemented).",  # pragma: no cover
        )

    def parse_binance_order_report_http(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        data: BinanceOrder,
        report_id: UUID4,
        ts_init: int,
    ) -> OrderStatusReport:
        client_order_id = ClientOrderId(data.clientOrderId) if data.clientOrderId != "" else None
        order_list_id = OrderListId(str(data.orderListId)) if data.orderListId is not None else None
        contingency_type = (
            ContingencyType.OCO
            if data.orderListId is not None and data.orderListId != -1
            else ContingencyType.NO_CONTINGENCY
        )

        trigger_price = Decimal(data.stopPrice)
        trigger_type = TriggerType.NO_TRIGGER
        if data.workingType is not None:
            trigger_type = self.parse_binance_trigger_type(data.workingType)
        elif trigger_price > 0:
            trigger_type = TriggerType.LAST_TRADE if trigger_price > 0 else TriggerType.NO_TRIGGER

        trailing_offset = None
        trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET
        if data.priceRate is not None:
            trailing_offset = Decimal(data.priceRate)
            trailing_offset_type = TrailingOffsetType.BASIS_POINTS

        avg_px = Decimal(data.avgPrice) if data.avgPrice is not None else None
        post_only = (
            data.type == BinanceOrderType.LIMIT_MAKER or data.timeInForce == BinanceTimeInForce.GTX
        )
        reduce_only = data.reduceOnly if data.reduceOnly is not None else False

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_list_id=order_list_id,
            venue_order_id=VenueOrderId(str(data.orderId)),
            order_side=OrderSide[data.side],
            order_type=self.parse_binance_order_type(data.type),
            contingency_type=contingency_type,
            time_in_force=self.parse_binance_time_in_force(data.timeInForce),
            order_status=self.parse_binance_order_status(data.status),
            price=Price.from_str(str(Decimal(data.price))),
            trigger_price=Price.from_str(str(trigger_price)),
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            quantity=Quantity.from_str(data.origQty),
            filled_qty=Quantity.from_str(data.executedQty),
            avg_px=avg_px,
            post_only=post_only,
            reduce_only=reduce_only,
            ts_accepted=millis_to_nanos(data.time),
            ts_last=millis_to_nanos(data.updateTime),
            report_id=report_id,
            ts_init=ts_init,
        )

    def parse_binance_trade_report_http(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        data: BinanceUserTrade,
        report_id: UUID4,
        ts_init: int,
    ) -> TradeReport:
        venue_position_id = None
        if data.positionSide is not None:
            venue_position_id = PositionId(f"{instrument_id}-{data.positionSide}")

        order_side = OrderSide.BUY if data.isBuyer or data.buyer else OrderSide.SELL
        liquidity_side = LiquiditySide.MAKER if data.isMaker or data.maker else LiquiditySide.TAKER

        return TradeReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(str(data.orderId)),
            venue_position_id=venue_position_id,
            trade_id=TradeId(str(data.id)),
            order_side=order_side,
            last_qty=Quantity.from_str(data.qty),
            last_px=Price.from_str(data.price),
            liquidity_side=liquidity_side,
            ts_event=millis_to_nanos(data.time),
            commission=Money(data.commission, Currency.from_str(data.commissionAsset)),
            report_id=report_id,
            ts_init=ts_init,
        )
