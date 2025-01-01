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

import json
from decimal import Decimal

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEnumParser
from nautilus_trader.adapters.okx.common.enums import OKXExecutionType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.adapters.okx.common.enums import OKXOrderStatus
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXSelfTradePreventionMode
from nautilus_trader.adapters.okx.common.enums import OKXTakeProfitKind
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXTransactionType
from nautilus_trader.adapters.okx.common.enums import OKXTriggerType
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# Place Order: POST /api/v5/trade/order
################################################################################


class OKXPlaceOrderData(msgspec.Struct):
    ordId: str
    clOrdId: str
    tag: str
    ts: str  # milliseconds when OKX finished order request processing
    sCode: str  # event code, "0" means success
    sMsg: str  # rejection or success message of event execution


class OKXPlaceOrderResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXPlaceOrderData]
    inTime: str  # milliseconds when request hit REST gateway
    outTime: str  # milliseconds when response leaves REST gateway


################################################################################
# Cancel order: POST /api/v5/trade/cancel-order
################################################################################
class OKXCancelOrderData(msgspec.Struct):
    ordId: str
    clOrdId: str
    ts: str  # milliseconds when OKX finished order request processing
    sCode: str  # event code, "0" means success
    sMsg: str  # rejection or success message of event execution


class OKXCancelOrderResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXCancelOrderData]
    inTime: str  # milliseconds when request hit REST gateway
    outTime: str  # milliseconds when response leaves REST gateway


################################################################################
# Amend Order: POST /api/v5/trade/amend-order
################################################################################


class OKXAmendOrderData(msgspec.Struct):
    ordId: str
    clOrdId: str
    ts: str  # milliseconds when OKX finished order request processing
    reqId: str  # Client Request ID as assigned by the client for order amendment
    sCode: str  # event code, "0" means success
    sMsg: str  # rejection or success message of event execution


class OKXAmendOrderResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXAmendOrderData]
    inTime: str  # milliseconds when request hit REST gateway
    outTime: str  # milliseconds when response leaves REST gateway


################################################################################
# Close Position: POST /api/v5/trade/close-position
################################################################################


class OKXClosePositionData(msgspec.Struct):
    instId: str
    posSide: OKXPositionSide
    clOrdId: str
    tag: str


class OKXClosePositionResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXClosePositionData]


################################################################################
# Order Details: GET /api/v5/trade/{order,orders-pending,orders-history-archive}
################################################################################


class OKXAttachAlgoOrds(msgspec.Struct):
    attachAlgoId: str
    attachAlgoClOrdId: str
    tpOrdKind: OKXTakeProfitKind
    tpTriggerPx: str
    tpOrdPx: str
    slTriggerPx: str
    slOrdPx: str
    sz: str
    amendPxOnTriggerType: str
    failCode: str
    failReason: str
    tpTriggerPxType: OKXTriggerType = OKXTriggerType.NONE
    slTriggerPxType: OKXTriggerType = OKXTriggerType.NONE


class OKXLinkedAlgoOrd(msgspec.Struct):
    algoId: str


class OKXOrderDetailsData(msgspec.Struct):
    instType: OKXInstrumentType
    instId: str
    tgtCcy: str
    ccy: str
    ordId: str
    clOrdId: str
    tag: str
    px: str
    pxUsd: str  # only for OPTION instrument types
    pxVol: str  # only for OPTION instrument types
    pxType: str  # only for OPTION instrument types
    sz: str
    pnl: str
    ordType: OKXOrderType
    side: OKXOrderSide
    posSide: OKXPositionSide
    tdMode: OKXTradeMode
    accFillSz: str  # unit is contracts for FUTURES/SWAP/OPTION
    fillPx: str  # or "" if not filled
    tradeId: str  # last traded ID
    fillSz: str  # unit is contracts for FUTURES/SWAP/OPTION
    fillTime: str  # last filled time
    avgPx: str
    state: OKXOrderStatus
    lever: str
    attachAlgoClOrdId: str
    tpTriggerPx: str
    tpOrdPx: str
    slTriggerPx: str
    slOrdPx: str
    attachAlgoOrds: list[OKXAttachAlgoOrds]
    linkedAlgoOrd: OKXLinkedAlgoOrd
    feeCcy: str
    fee: str
    rebateCcy: str
    source: str
    """source: 6 (triggered order), 7 (triggered by tp/sl order), 13 (triggered by algo order),
    25 (triggered by trailing stop order, i.e. algo ordType=='move_order_stop')
    """
    rebate: str
    category: str
    reduceOnly: str  # "true" or "false"
    isTpLimit: str  # "true" or "false"
    cancelSource: str  # code of cancellation source
    cancelSourceReason: str
    quickMgnType: str
    algoClOrdId: str
    algoId: str
    uTime: str  # update time, milliseconds
    cTime: str  # creation time, milliseconds
    stpMode: OKXSelfTradePreventionMode = OKXSelfTradePreventionMode.NONE
    tpTriggerPxType: OKXTriggerType = OKXTriggerType.NONE
    slTriggerPxType: OKXTriggerType = OKXTriggerType.NONE

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        enum_parser: OKXEnumParser,
        ts_init: int,
        client_order_id: ClientOrderId | None,
    ) -> OrderStatusReport:
        """
        Create an order status report from the order message.
        """
        trigger_price = None
        trigger_type = TriggerType.NO_TRIGGER
        if self.tpTriggerPx:
            trigger_price = self.tpTriggerPx
            trigger_type = enum_parser.parse_okx_trigger_type(self.tpTriggerPxType)
        elif self.slTriggerPx:
            trigger_price = self.slTriggerPx
            trigger_type = enum_parser.parse_okx_trigger_type(self.slTriggerPxType)

        cancel_reason = (
            f"{self.cancelSource}: {self.cancelSourceReason}" if self.cancelSource else None
        )

        # TODO: assess self.attachAlgoOrds to support more nautilus order types
        order_type = enum_parser.parse_okx_order_type(self.ordType)

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(self.ordId),
            client_order_id=ClientOrderId(self.clOrdId) if self.clOrdId else client_order_id,
            report_id=report_id,
            order_list_id=None,
            contingency_type=ContingencyType.NO_CONTINGENCY,
            order_side=enum_parser.parse_okx_order_side(self.side),
            order_type=order_type,
            time_in_force=enum_parser.parse_okx_time_in_force(self.ordType),
            order_status=enum_parser.parse_okx_order_status(order_type, self.state),
            quantity=Quantity.from_str(self.sz),
            filled_qty=Quantity.from_str(self.accFillSz or "0.0"),
            display_qty=None,
            price=Price.from_str(self.px) if self.px else None,
            avg_px=Decimal(self.avgPx) if self.avgPx else None,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            limit_offset=None,
            trailing_offset=None,
            trailing_offset_type=TrailingOffsetType.NO_TRAILING_OFFSET,
            post_only=self.ordType == OKXOrderType.POST_ONLY,
            reduce_only=json.loads(self.reduceOnly),
            cancel_reason=cancel_reason,
            expire_time=None,
            ts_triggered=None,
            ts_accepted=millis_to_nanos(Decimal(self.cTime)),
            ts_last=millis_to_nanos(Decimal(self.uTime)),
            ts_init=ts_init,
        )


class OKXOrderDetailsResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXOrderDetailsData]


################################################################################
# Order Details: GET /api/v5/trade/fills-history (up to last 3 months)
################################################################################


class OKXFillsHistoryData(msgspec.Struct):
    instType: OKXInstrumentType
    instId: str
    tradeId: str
    ordId: str
    clOrdId: str
    billId: str
    subType: OKXTransactionType
    tag: str
    fillPx: str
    fillSz: str
    fillIdxPx: str  # index price at moment of execution
    fillPnl: str
    # Last filled pnl, applicable to orders with a trade and aim to close position. It always is 0
    # in other conditions
    fillPxVol: str  # OPTION instruments
    fillPxUsd: str  # OPTION instruments
    fillMarkVol: str  # OPTION instruments
    fillFwdPx: str  # OPTION instruments
    fillMarkPx: str  # OPTION instruments
    side: OKXOrderSide
    posSide: OKXPositionSide
    execType: OKXExecutionType
    feeCcy: str
    fee: str  # negative is charged, positive is rebate
    ts: str
    fillTime: str  # Unix time in milliseconds

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        client_order_id: ClientOrderId | None,
        enum_parser: OKXEnumParser,
        ts_init: int,
    ) -> FillReport:
        fee_currency = Currency.from_str(self.feeCcy or "USDT")
        fee = format(float(self.fee or 0), f".{fee_currency.precision}f")
        commission = Money(Decimal(fee), fee_currency)

        client_order_id = (
            client_order_id
            if client_order_id
            else ClientOrderId(self.clOrdId) if self.clOrdId else None
        )

        return FillReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(str(self.ordId)),
            trade_id=TradeId(self.tradeId),
            client_order_id=client_order_id,
            order_side=enum_parser.parse_okx_order_side(self.side),
            last_qty=Quantity.from_str(self.fillSz),
            last_px=Price.from_str(self.fillPx),
            commission=commission,
            liquidity_side=self.execType.parse_to_liquidity_side(),
            report_id=report_id,
            ts_event=millis_to_nanos(Decimal(self.fillTime)),
            ts_init=ts_init,
        )


class OKXFillsHistoryResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXFillsHistoryData]


################################################################################
# Order Details: GET /api/v5/trade/fills (up to last 3 days)
################################################################################


class OKXFillsData(msgspec.Struct):
    instType: OKXInstrumentType
    instId: str
    tradeId: str
    ordId: str
    clOrdId: str
    billId: str
    subType: OKXTransactionType
    tag: str
    fillPx: str
    fillSz: str
    fillIdxPx: str  # index price at moment of execution
    fillPnl: str
    # Last filled pnl, applicable to orders with a trade and aim to close position. It always is 0
    # in other conditions
    fillPxVol: str  # OPTION instruments
    fillPxUsd: str  # OPTION instruments
    fillMarkVol: str  # OPTION instruments
    fillFwdPx: str  # OPTION instruments
    fillMarkPx: str  # OPTION instruments
    side: OKXOrderSide
    posSide: OKXPositionSide
    execType: OKXExecutionType
    feeCcy: str
    fee: str  # negative is charged, positive is rebate
    ts: str
    fillTime: str  # Unix time in milliseconds

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        client_order_id: ClientOrderId,
        enum_parser: OKXEnumParser,
        ts_init: int,
    ) -> FillReport:
        fee_currency = Currency.from_str(self.feeCcy or "USDT")
        fee = format(float(self.fee or 0), f".{fee_currency.precision}f")
        commission = Money(Decimal(fee), fee_currency)

        return FillReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(str(self.ordId)),
            trade_id=TradeId(self.tradeId),
            client_order_id=client_order_id,
            order_side=enum_parser.parse_okx_order_side(self.side),
            last_qty=Quantity.from_str(self.fillSz),
            last_px=Price.from_str(self.fillPx),
            commission=commission,
            liquidity_side=self.execType.parse_to_liquidity_side(),
            report_id=report_id,
            ts_event=millis_to_nanos(Decimal(self.fillTime)),
            ts_init=ts_init,
        )


class OKXFillsResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXFillsData]
