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

from decimal import Decimal
from typing import Any

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitStopOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitOrder(msgspec.Struct, omit_defaults=True, kw_only=True):
    orderId: str
    orderLinkId: str
    blockTradeId: str | None = None
    symbol: str
    price: str
    qty: str
    side: BybitOrderSide
    isLeverage: str
    positionIdx: int
    orderStatus: BybitOrderStatus
    cancelType: str
    rejectReason: str
    avgPrice: str
    leavesQty: str
    leavesValue: str
    cumExecQty: str
    cumExecValue: str
    cumExecFee: str
    timeInForce: BybitTimeInForce
    orderType: BybitOrderType
    stopOrderType: BybitStopOrderType | None = None
    orderIv: str
    triggerPrice: str
    takeProfit: str
    stopLoss: str
    tpTriggerBy: str
    slTriggerBy: str
    triggerDirection: int
    triggerBy: BybitTriggerType | None = None
    lastPriceOnCreated: str
    reduceOnly: bool
    closeOnTrigger: bool
    smpType: str
    smpGroup: int
    smpOrderId: str
    tpslMode: str | None = None
    tpLimitPrice: str
    slLimitPrice: str
    placeType: str
    createdTime: str
    updatedTime: str

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        enum_parser: BybitEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        client_order_id = ClientOrderId(self.orderLinkId) if self.orderLinkId else None
        order_list_id = None
        contingency_type = ContingencyType.NO_CONTINGENCY
        trigger_price = (
            Price.from_str(str(Decimal(self.triggerPrice))) if self.triggerPrice else None
        )
        trigger_type = TriggerType.NO_TRIGGER
        # TODO check for trigger type
        trailing_offset = None
        trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET
        order_status = enum_parser.parse_bybit_order_status(self.orderStatus)
        # check for post only and reduce only
        post_only = False
        reduce_only = False
        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            order_list_id=order_list_id,
            venue_order_id=VenueOrderId(str(self.orderId)),
            order_side=enum_parser.parse_bybit_order_side(self.side),
            order_type=enum_parser.parse_bybit_order_type(self.orderType),
            contingency_type=contingency_type,
            time_in_force=enum_parser.parse_bybit_time_in_force(self.timeInForce),
            order_status=order_status,
            price=Price.from_str(self.price),
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            quantity=Quantity.from_str(self.qty),
            filled_qty=Quantity.from_str(self.cumExecQty),
            avg_px=Decimal(self.avgPrice) if self.avgPrice else None,
            post_only=post_only,
            reduce_only=reduce_only,
            ts_accepted=millis_to_nanos(Decimal(self.createdTime)),
            ts_last=millis_to_nanos(Decimal(self.updatedTime)),
            report_id=report_id,
            ts_init=ts_init,
        )


class BybitOpenOrdersResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitOrder]
    time: int


class BybitOrderHistoryResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitOrder]
    time: int


################################################################################
# Place Order
################################################################################


class BybitPlaceOrder(msgspec.Struct):
    orderId: str
    orderLinkId: str


class BybitPlaceOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitPlaceOrder
    time: int


################################################################################
# Cancel order
################################################################################
class BybitCancelOrder(msgspec.Struct):
    orderId: str
    orderLinkId: str


class BybitCancelOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitCancelOrder
    time: int


################################################################################
# Cancel All Orders
################################################################################
class BybitCancelAllOrders(msgspec.Struct):
    orderId: str
    orderLinkId: str


class BybitCancelAllOrdersResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult[BybitCancelAllOrders]
    time: int


################################################################################
# Amend order
################################################################################
class BybitAmendOrder(msgspec.Struct):
    orderId: str
    orderLinkId: str


class BybitAmendOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitAmendOrder
    retExtInfo: dict[str, Any]
    time: int


################################################################################
# Batch place order
################################################################################


class BybitPlaceResult(msgspec.Struct):
    code: int  # Success/error code
    msg: str  # Success/error message


class BybitBatchPlaceOrderExtInfo(msgspec.Struct):
    list: list[BybitPlaceResult]


class BybitBatchPlaceOrder(msgspec.Struct):
    category: BybitProductType
    symbol: str
    orderId: str
    orderLinkId: str
    createAt: str


class BybitBatchPlaceOrderResult(msgspec.Struct):
    list: list[BybitBatchPlaceOrder]


class BybitBatchPlaceOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitBatchPlaceOrderResult
    retExtInfo: BybitBatchPlaceOrderExtInfo
    time: int


################################################################################
# Batch cancel order
################################################################################


class BybitCancelResult(msgspec.Struct):
    code: int  # Success/error code
    msg: str  # Success/error message


class BybitBatchCancelOrderExtInfo(msgspec.Struct):
    list: list[BybitCancelResult]


class BybitBatchCancelOrder(msgspec.Struct):
    category: BybitProductType
    symbol: str
    orderId: str
    orderLinkId: str


class BybitBatchCancelOrderResult(msgspec.Struct):
    list: list[BybitBatchCancelOrder]


class BybitBatchCancelOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitBatchCancelOrderResult
    retExtInfo: BybitBatchCancelOrderExtInfo
    time: int


################################################################################
# Batch amend order
################################################################################


class BybitAmendResult(msgspec.Struct):
    code: int  # Success/error code
    msg: str  # Success/error message


class BybitBatchAmendOrderExtInfo(msgspec.Struct):
    list: list[BybitAmendResult]


class BybitBatchAmendOrder(msgspec.Struct):
    category: BybitProductType
    symbol: str
    orderId: str
    orderLinkId: str


class BybitBatchAmendOrderResult(msgspec.Struct):
    list: list[BybitBatchAmendOrder]


class BybitBatchAmendOrderResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitBatchAmendOrderResult
    retExtInfo: BybitBatchAmendOrderExtInfo
    time: int
