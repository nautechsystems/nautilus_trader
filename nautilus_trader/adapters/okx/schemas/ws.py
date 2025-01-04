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
from typing import Literal

import msgspec

from nautilus_trader.adapters.okx.common.enums import OKXEnumParser
from nautilus_trader.adapters.okx.common.enums import OKXExecutionType
from nautilus_trader.adapters.okx.common.enums import OKXInstrumentType
from nautilus_trader.adapters.okx.common.enums import OKXMarginMode
from nautilus_trader.adapters.okx.common.enums import OKXOrderSide
from nautilus_trader.adapters.okx.common.enums import OKXOrderStatus
from nautilus_trader.adapters.okx.common.enums import OKXOrderType
from nautilus_trader.adapters.okx.common.enums import OKXPositionSide
from nautilus_trader.adapters.okx.common.enums import OKXSelfTradePreventionMode
from nautilus_trader.adapters.okx.common.enums import OKXTakeProfitKind
from nautilus_trader.adapters.okx.common.enums import OKXTradeMode
from nautilus_trader.adapters.okx.common.enums import OKXTriggerType
from nautilus_trader.adapters.okx.common.parsing import parse_aggressor_side
from nautilus_trader.adapters.okx.common.parsing import parse_okx_ws_delta
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.book import OrderBook
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import ContingencyType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import TrailingOffsetType
from nautilus_trader.model.enums import TriggerType
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Currency
from nautilus_trader.model.objects import MarginBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# General message structs
################################################################################


class OKXWsGeneralMsg(msgspec.Struct, frozen=True):
    event: str | None = None  # event message
    data: list | None = None  # push data
    id: str | None = None  # order message
    op: str | None = None  # order message
    algoId: str | None = None  # algo order message

    @property
    def is_event_msg(self) -> bool:
        return self.event is not None

    @property
    def is_push_data_msg(self) -> bool:
        return self.data is not None and self.id is None

    @property
    def is_order_msg(self) -> bool:
        return self.id is not None and self.op is not None and "order" in self.op

    @property
    def is_algo_order_msg(self) -> bool:
        return self.algoId is not None


class OKXWsEventMsgArg(msgspec.Struct, frozen=True):
    channel: str
    ccy: str | None = None
    instType: str | None = None
    instFamily: str | None = None
    instId: str | None = None


class OKXWsEventMsg(msgspec.Struct, frozen=True):
    event: str
    connId: str
    code: str | None = None
    msg: str | None = None
    arg: OKXWsEventMsgArg | None = None
    channel: str | None = None
    connCount: str | None = None

    @property
    def is_channel_conn_count_error(self) -> bool:
        return self.event == "channel-conn-count-error"

    @property
    def is_error(self) -> bool:
        return "error" in self.event

    @property
    def is_login(self) -> bool:
        return self.event == "login"

    @property
    def is_subscribe_unsubscribe(self) -> bool:
        return self.event == "subscribe" or self.event == "unsubscribe"

    def format_channel_conn_count_error(self) -> str:
        assert self.is_channel_conn_count_error
        return msgspec.json.encode(self).decode()

    def format_error(self) -> str:
        assert self.is_error
        return msgspec.json.encode(self).decode()


class OKXWsPushDataArg(msgspec.Struct):
    channel: str
    uid: str | None = None  # user id
    instType: str | None = None
    instFamily: str | None = None
    instId: str | None = None
    algoId: str | None = None


class OKXWsPushDataMsg(msgspec.Struct):
    arg: OKXWsPushDataArg
    data: list | None = None
    action: str | None = None  # for orderbook subscriptions: can be 'snapshot' or 'update'


################################################################################
# Public - Orderbook
################################################################################


class OKXWsOrderbookData(msgspec.Struct):
    asks: list[list[str]]
    bids: list[list[str]]
    ts: str
    seqId: int
    checksum: int | None = None
    prevSeqId: int | None = None

    def parse_to_snapshot(  # if OKXWsOrderbookPushDataResponse.action == 'snapshot'
        self,
        instrument_id: InstrumentId,
        price_precision: int | None,
        size_precision: int | None,
        ts_init: int,
    ) -> OrderBookDeltas:
        ts_event = millis_to_nanos(Decimal(self.ts))

        bids_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.bids
        ]
        asks_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.asks
        ]
        deltas: list[OrderBookDelta] = []

        # Add initial clear
        clear = OrderBookDelta.clear(
            instrument_id=instrument_id,
            sequence=self.seqId,
            ts_event=ts_event,
            ts_init=ts_init,
        )
        deltas.append(clear)

        bids_len = len(bids_raw)
        asks_len = len(asks_raw)

        for idx, bid in enumerate(bids_raw):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_okx_ws_delta(
                instrument_id=instrument_id,
                values=bid,
                side=OrderSide.BUY,
                sequence=self.seqId,
                ts_event=ts_event,
                ts_init=ts_init,
                is_snapshot=True,
                flags=flags,
            )
            deltas.append(delta)

        for idx, ask in enumerate(asks_raw):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_okx_ws_delta(
                instrument_id=instrument_id,
                values=ask,
                side=OrderSide.SELL,
                sequence=self.seqId,
                ts_event=ts_event,
                ts_init=ts_init,
                is_snapshot=True,
                flags=flags,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)

    def parse_to_deltas(  # if OKXWsOrderbookPushDataResponse.action == 'update'
        self,
        instrument_id: InstrumentId,
        price_precision: int | None,
        size_precision: int | None,
        ts_init: int,
    ) -> OrderBookDeltas:
        ts_event = millis_to_nanos(Decimal(self.ts))

        bids_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.bids
        ]
        asks_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.asks
        ]
        deltas: list[OrderBookDelta] = []

        bids_len = len(bids_raw)
        asks_len = len(asks_raw)

        for idx, bid in enumerate(bids_raw):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_okx_ws_delta(
                instrument_id=instrument_id,
                values=bid,
                side=OrderSide.BUY,
                sequence=self.seqId,
                ts_event=ts_event,
                ts_init=ts_init,
                is_snapshot=False,
                flags=flags,
            )
            deltas.append(delta)

        for idx, ask in enumerate(asks_raw):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_okx_ws_delta(
                instrument_id=instrument_id,
                values=ask,
                side=OrderSide.SELL,
                sequence=self.seqId,
                ts_event=ts_event,
                ts_init=ts_init,
                is_snapshot=False,
                flags=flags,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)

    def parse_to_quote_tick(
        self,
        instrument_id: InstrumentId,
        price_precision: int | None,
        size_precision: int | None,
        last_quote: QuoteTick | None,
        ts_init: int,
    ) -> QuoteTick | None:
        if last_quote is None and (not self.bids or not self.asks):
            return None
        top_bid = self.bids[0] if self.bids else None
        top_ask = self.asks[0] if self.asks else None
        top_bid_price = top_bid[0] if top_bid else None
        top_ask_price = top_ask[0] if top_ask else None
        top_bid_size = top_bid[1] if top_bid else None
        top_ask_size = top_ask[1] if top_ask else None

        if top_bid_size == "0":
            top_bid_size = None
        if top_ask_size == "0":
            top_ask_size = None

        if last_quote is None and (top_bid_size is None or top_ask_size is None):
            return None

        if last_quote is None:
            assert top_bid_price and top_ask_price and top_bid_size and top_ask_size
            bid_price = Price(float(top_bid_price), price_precision)
            ask_price = Price(float(top_ask_price), price_precision)
            bid_size = Quantity(float(top_bid_size), size_precision)
            ask_size = Quantity(float(top_ask_size), size_precision)

        if not top_bid_price:
            assert last_quote is not None
            bid_price = last_quote.bid_price
        if not top_ask_price:
            assert last_quote is not None
            ask_price = last_quote.ask_price
        if not top_bid_size:
            assert last_quote is not None
            bid_size = last_quote.bid_size
        if not top_ask_size:
            assert last_quote is not None
            ask_size = last_quote.ask_size

        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=millis_to_nanos(Decimal(self.ts)),
            ts_init=ts_init,
        )

    def parse_to_quote_tick_from_book_and_deltas(
        self,
        instrument_id: InstrumentId,
        book: OrderBook,
        deltas: OrderBookDeltas,
        last_quote: QuoteTick | None,
        ts_init: int,
    ) -> QuoteTick | None:
        book.apply_deltas(deltas)

        bid_price = book.best_bid_price()
        ask_price = book.best_ask_price()
        bid_size = book.best_bid_size()
        ask_size = book.best_ask_size()

        if bid_price is None and last_quote is not None:
            bid_price = last_quote.bid_price

        if ask_price is None and last_quote is not None:
            ask_price = last_quote.ask_price

        if bid_size is None and last_quote is not None:
            bid_size = last_quote.bid_size

        if ask_size is None and last_quote is not None:
            ask_size = last_quote.ask_size

        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=bid_price,
            ask_price=ask_price,
            bid_size=bid_size,
            ask_size=ask_size,
            ts_event=deltas.ts_event,
            ts_init=deltas.ts_init,
        )


class OKXWsOrderbookArg(msgspec.Struct):
    channel: str
    instId: str


class OKXWsOrderbookPushDataMsg(msgspec.Struct):
    arg: OKXWsOrderbookArg
    data: list[OKXWsOrderbookData]
    action: Literal["snapshot", "update"] | None = None


def decoder_ws_orderbook() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsOrderbookPushDataMsg)


################################################################################
# Public - Trades
################################################################################


class OKXWsTradeData(msgspec.Struct):
    instId: str
    tradeId: str
    px: str
    sz: str
    side: str
    ts: str
    count: str

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        price_precision: int | None,
        size_precision: int | None,
        ts_init: int,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id,
            price=Price(float(self.px), price_precision),
            size=Quantity(float(self.sz), size_precision),
            aggressor_side=parse_aggressor_side(self.side),
            trade_id=TradeId(self.tradeId),
            ts_event=millis_to_nanos(Decimal(self.ts)),
            ts_init=ts_init,
        )


class OKXWsTradesArg(msgspec.Struct):
    channel: str
    instId: str


class OKXWsTradesPushDataMsg(msgspec.Struct):
    arg: OKXWsTradesArg
    data: list[OKXWsTradeData]


def decoder_ws_trade() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsTradesPushDataMsg)


################################################################################
# Private - Order execution: /place-order, /amend-order, /cancel-order
################################################################################


class OKXWsOrderMsgData(msgspec.Struct):
    ordId: str
    clOrdId: str
    ts: str
    sCode: str
    sMsg: str
    tag: str | None = None

    @property
    def rejection_reason(self) -> str:
        if self.sCode != "0":
            return f"{self.sCode}: {self.sMsg}"
        return ""


class OKXWsOrderMsg(msgspec.Struct):
    id: str
    op: str
    code: str
    msg: str
    data: list[OKXWsOrderMsgData]
    inTime: str
    outTime: str


def decoder_ws_order() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsOrderMsg)


# TODO
################################################################################
# Private - Orders
################################################################################


class OKXAttachAlgoOrds(msgspec.Struct):
    attachAlgoId: str
    attachAlgoClOrdId: str
    tpOrdKind: OKXTakeProfitKind
    tpTriggerPx: str
    tpTriggerPxType: OKXTriggerType
    tpOrdPx: str
    slTriggerPx: str
    slTriggerPxType: OKXTriggerType
    slOrdPx: str
    sz: str
    amendPxOnTriggerType: str


class OKXLinkedAlgoOrd(msgspec.Struct):
    algoId: str  # says object in docs (https://www.okx.com/docs-v5/en/#order-book-trading-trade-ws-order-channel) but is str in example


WS_CANCEL_SOURCE_REASONS = {
    "0": "Order canceled by system",
    "1": "Order canceled by user",
    "2": "Order canceled: Pre reduce-only order canceled, due to insufficient margin in user "
    "position",
    "3": "Order canceled: Risk cancellation was triggered. Pending order was canceled due to "
    "insufficient margin ratio and forced-liquidation risk.",
    "4": "Order canceled: Borrowings of crypto reached hard cap, order was canceled by system.",
    "6": "Order canceled: ADL order cancellation was triggered. Pending order was canceled due to "
    "a low margin ratio and forced-liquidation risk.",
    "7": "Order canceled: Futures contract delivery.",
    "9": "Order canceled: Insufficient balance after funding fees deducted.",
    "13": "Order canceled: FOK order was canceled due to incompletely filled.",
    "14": "Order canceled: IOC order was partially canceled due to incompletely filled.",
    "15": "Order canceled: The order price is beyond the limit",
    "17": "Order canceled: Close order was canceled, due to the position was already closed at "
    "market price.",
    "20": "Cancel all after triggered",
    "21": "Order canceled: The TP/SL order was canceled because the position had been closed",
    "22": "Order canceled: Reduce-only orders only allow reducing your current position. System "
    "has already canceled this order.",
    "23": "Order canceled: Reduce-only orders only allow reducing your current position. System "
    "has already canceled this order.",
    "27": "Order canceled: Price limit verification failed because the price difference between "
    "counterparties exceeds 5%",
    "31": "The post-only order will take liquidity in taker orders",
    "32": "Self trade prevention",
    "33": "The order exceeds the maximum number of order matches per taker order",
    "36": "Your TP limit order was canceled because the corresponding SL order was triggered.",
    "37": "Your TP limit order was canceled because the corresponding SL order was canceled.",
    "38": "You have canceled market maker protection (MMP) orders.",
    "39": "Your order was canceled because market maker protection (MMP) was triggered.",
}
WS_AMEND_SOURCE_REASONS = {
    "1": "Order amended by user",
    "2": "Order amended by user, but the order quantity is overridden by system due to reduce-only",
    "3": "New order placed by user, but the order quantity is overridden by system due to "
    "reduce-only",
    "4": "Order amended by system due to other pending orders",
    "5": "Order modification due to changes in options px, pxVol, or pxUsd as a result of "
    "following variations. For example, when iv = 60, usd and px are anchored at iv = 60, the "
    "changes in usd or px lead to modification.",
}
WS_AMEND_RESULT_REASONS = {
    "-1": "failure",
    "0": "success",
    "1": "Automatic cancel (amendment request returned success but amendment subsequently failed "
    "then automatically canceled by the system)",
    "2": "Automatic amendation successfully, only applicable to pxVol and pxUsd orders of Option.",
}


class OKXWsOrdersData(msgspec.Struct):
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
    notionalUsd: str  # order notional value in USD
    ordType: OKXOrderType
    side: OKXOrderSide
    posSide: OKXPositionSide
    tdMode: OKXTradeMode
    fillPx: str  # last filled price
    tradeId: str
    fillSz: str  # unit is contracts for FUTURES/SWAP/OPTION
    fillPnl: str  # Last filled pnl, applicable to orders with a trade aiming to close position. It always is 0 in other conditions
    fillTime: str  # last filled time
    fillFee: str  # last filled fee amt, negative is charge, positive is rebate
    fillFeeCcy: str  # fee currency when fillFee is negative, else rebate currency
    fillPxVol: str  # only for OPTION instrument types
    fillPxUsd: str  # only for OPTION instrument types
    fillMarkVol: str  # only for OPTION instrument types
    fillFwdPx: str  # only for OPTION instrument types
    execType: OKXExecutionType
    accFillSz: str  # unit is contracts for FUTURES/SWAP/OPTION
    fillNotionalUsd: str  # filled notional value in USD
    avgPx: str  # avg filled price, 0 if none filled
    state: OKXOrderStatus
    lever: str
    attachAlgoClOrdId: str
    tpTriggerPx: str
    tpTriggerPxType: OKXTriggerType
    tpOrdPx: str
    slTriggerPx: str
    slTriggerPxType: OKXTriggerType
    slOrdPx: str
    attachAlgoOrds: list[OKXAttachAlgoOrds]
    linkedAlgoOrd: OKXLinkedAlgoOrd
    stpId: str  # DEPRECATED
    stpMode: OKXSelfTradePreventionMode
    feeCcy: str
    fee: str  # accumulated fee for futures, perps, and options
    rebateCcy: str  # "" if there is no rebate
    rebate: str  # rebate accumulated amount, applicable to spot and margin, "" if no rebate
    pnl: str
    source: str
    """source:
    6 (the normal order triggered by trigger order),
    7 (the normal order triggered by tp/sl order),
    13 (the normal order triggered by algo order),
    25 (the normal order  triggered by trailing stop order, i.e. algo ordType=='move_order_stop')
    """
    cancelSource: str
    amendSource: str
    category: str  # normal/twap/adl/full_liquidation/partial_liquidation/delivery/ddh
    isTpLimit: str  # whether it is a take profit limit order, "true" or "false"
    uTime: str  # update time, milliseconds
    cTime: str  # creation time, milliseconds
    reqId: str  # Client Request ID as assigned by client for order amendment. "" if there is no order amendment.
    amendResult: str
    reduceOnly: str  # "true" or "false"
    quickMgnType: str  # manual/auto_borrow/auto_repay, only applicable to Quick Margin Mode of isolated margin
    algoClOrdId: str
    algoId: str
    code: str
    msg: str

    @property
    def is_amended(self) -> bool:
        return bool(self.amendSource) or bool(self.amendResult)

    @property
    def is_canceled(self) -> bool:
        return bool(self.cancelSource)

    @property
    def amend_source_reason(self) -> str | None:
        return (
            f"AMEND_SOURCE[{self.amendSource}: {WS_AMEND_SOURCE_REASONS[self.amendSource]}]"
            if self.amendSource
            else None
        )

    @property
    def amend_result_reason(self) -> str | None:
        return (
            f"AMEND_RESULT[{self.amendResult}: {WS_AMEND_RESULT_REASONS[self.amendResult]}]"
            if self.amendResult
            else None
        )

    @property
    def cancel_reason(self) -> str | None:
        return (
            f"{self.cancelSource}: {WS_CANCEL_SOURCE_REASONS[self.cancelSource]}"
            if self.cancelSource
            else None
        )

    def get_fill_px(self, price_precision: int | None = None) -> Price:
        if price_precision:
            return Price(float(self.fillPx), price_precision)
        return Price.from_str(self.fillPx)

    def get_fill_sz(self, size_precision: int | None = None) -> Quantity:
        if size_precision:
            return Quantity(float(self.fillSz), size_precision)
        return Quantity.from_str(self.fillSz)

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
            trigger_price = Price.from_str(self.tpTriggerPx)
            trigger_type = enum_parser.parse_okx_trigger_type(self.tpTriggerPxType)
        elif self.slTriggerPx:
            trigger_price = Price.from_str(self.slTriggerPx)
            trigger_type = enum_parser.parse_okx_trigger_type(self.slTriggerPxType)

        cancel_reason = (
            f"{self.cancelSource}: {WS_CANCEL_SOURCE_REASONS[self.cancelSource]}"
            if self.cancelSource
            else None
        )

        # TODO: assess self.attachAlgoOrds to support more nautilus order types
        order_type = enum_parser.parse_okx_order_type(self.ordType)

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            venue_order_id=VenueOrderId(self.ordId),
            client_order_id=client_order_id,
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


class OKXWsOrdersArg(msgspec.Struct):
    channel: str
    uid: str
    instType: OKXInstrumentType
    instFamily: str | None = None
    instId: str | None = None


class OKXWsOrdersPushDataMsg(msgspec.Struct):
    arg: OKXWsOrdersArg
    data: list[OKXWsOrdersData]


def decoder_ws_orders() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsOrdersPushDataMsg)


################################################################################
# Private - Account
################################################################################


class OKXAssetInformationDetails(msgspec.Struct, dict=True):
    ccy: str
    eq: str  # equity of currency
    cashBal: str
    uTime: str  # update time of currency balance
    isoEq: str  # isolated margin equity of currency
    availEq: str  # available equity of currency
    disEq: str  # discount equity of currency in USD
    fixedBal: str  # Frozen balance for Dip Sniper and Peak Sniper
    availBal: str  # available balance of currency
    frozenBal: str  # frozen balance of currency
    ordFrozen: str  # frozen margin for open orders
    liab: str  # liabilities of currency
    upl: str  # sum of unrealized profit & loss of all margin and derivative positions of currency
    uplLiab: str  # liabilities due to unrealized loss of currency
    crossLiab: str  # cross liabilities of currency
    rewardBal: str  # trial fund balance
    isoLiab: str  # isolated liabilities of currency
    mgnRatio: str  # margin ratio of currency
    interest: str  # accrued interest of currency
    twap: str  # risk auto liability payment, 0-5 in increasing risk of auto payment trigger risk
    maxLoan: str  # max loan of currency
    eqUsd: str  # equity of currency in USD
    borrowFroz: str  # potential borrowing IMR of currency in USD
    notionalLever: str  # leverage of currency
    stgyEq: str  # strategy equity
    isoUpl: str  # isolated unrealized profit & loss of currency
    spotInUseAmt: str  # spot in use amount
    clSpotInUseAmt: str  # user-defined spot risk offset amount
    maxSpotInUseAmt: str  # Max possible spot risk offset amount
    spotIsoBal: str  # spot isolated balance, applicable to copy trading, applicable to spot/futures
    imr: str  # initial margin requirement at the currency level, applicable to spot/futures
    mmr: str  # maintenance margin requirement at the currency level, applicable to spot/futures
    smtSyncEq: str  # smart sync equity, The default is "0", only applicable to copy trader

    def parse_to_account_balance(self) -> AccountBalance | None:
        if not self.eq or not self.availEq:
            return None

        currency = Currency.from_str(self.ccy)
        format_spec = f".{currency.precision}f"
        total = Decimal(format(float(self.eq), format_spec))
        free = Decimal(format(float(self.availEq), format_spec))
        locked = total - free

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance | None:
        if not self.imr or not self.mmr:
            return None

        currency: Currency = Currency.from_str(self.ccy)
        format_spec = f".{currency.precision}f"
        imr = Decimal(format(float(self.imr), format_spec))
        mmr = Decimal(format(float(self.mmr), format_spec))

        return MarginBalance(
            initial=Money(imr, currency),
            maintenance=Money(mmr, currency),
        )


class OKXWsAccountData(msgspec.Struct):
    uTime: str  # update time of account information
    totalEq: str  # total USD
    isoEq: str  # isolated margin in USD
    adjEq: str  # net USD value of all assets contributing to margin rqts, discounted for mkt risk
    ordFroz: str  # cross-margin USD frozen for pending orders
    imr: str  # total USD init margins of all open positions & pending orders in cross-margin mode
    mmr: str  # total USD maint margins of all open positions & pending orders in cross-margin mode
    borrowFroz: str  # potential borrowing IMR in USD
    mgnRatio: str  # margin ratio in USD
    notionalUsd: str  # notional value of positions in USD
    upl: str  # cross-margin info of unrealized profit & loss at account level in USD
    details: list[OKXAssetInformationDetails]

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str("USD")
        format_spec = f".{currency.precision}f"
        total = Decimal(format(float(self.totalEq), format_spec))
        free = Decimal(format(float(self.adjEq), format_spec))

        locked = total - free

        return AccountBalance(
            total=Money(total, currency),
            locked=Money(locked, currency),
            free=Money(free, currency),
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency: Currency = Currency.from_str("USD")
        format_spec = f".{currency.precision}f"
        imr = Decimal(format(float(self.imr), format_spec))
        mmr = Decimal(format(float(self.mmr), format_spec))

        return MarginBalance(
            initial=Money(imr, currency),
            maintenance=Money(mmr, currency),
        )


class OKXWsAccountArg(msgspec.Struct):
    channel: str
    uid: str


class OKXWsAccountPushDataMsg(msgspec.Struct):
    arg: OKXWsAccountArg
    data: list[OKXWsAccountData]


def decoder_ws_account() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsAccountPushDataMsg)


# TODO
################################################################################
# Private - Positions
################################################################################


class OKXCloseOrderAlgoData(msgspec.Struct):
    algoId: str
    slTriggerPx: str
    slTriggerPxType: OKXTriggerType
    tpTriggerPx: str
    tpTriggerPxType: OKXTriggerType
    closeFraction: str  # fraction of position to be closed when algo order is triggered


class OKXWsPositionsData(msgspec.Struct):
    instType: OKXInstrumentType
    mgnMode: OKXMarginMode
    posId: str
    posSide: OKXPositionSide
    pos: str  # qty of positions
    baseBal: str  # DEPRECATED
    quoteBal: str  # DEPRECATED
    baseBorrowed: str  # DEPRECATED
    baseInterest: str  # DEPRECATED
    quoteBorrowed: str  # DEPRECATED
    quoteInterest: str  # DEPRECATED
    posCcy: str  # position currency applicable to margin positions
    availPos: str  # position that can be closed, applicable to MARGIN/FUTURES/SWAP long/short mode
    avgPx: str  # avg open price
    upl: str  # unrealized pnl calculated by mark price
    uplRatio: str  # unrealized pnl ratio calc'd by mark price
    uplLastPx: str  # Unrealized pbl calculated by last price. For show, actual value is upl
    uplRatioLastPx: str  # unrealized pnl ratio calc'd by last price
    instId: str
    lever: str  # leverage
    liqPx: str  # estimated liquidation price
    markPx: str  # latest mark price
    imr: str  # init margin rqt, only applicable to 'cross'
    margin: str  # margin, can be added or reduced, only applicable to 'isolated'
    mgnRatio: str  # margin ratio
    mmr: str  # maint margin rqt
    liab: str  # liabilities, only applicable to MARGIN
    liabCcy: str  # liabilities currency, only applicable to MARGIN
    interest: str  # interest. Undeducted interest that has been incurred
    tradeId: str  # last trade id
    notionalUsd: str  # notional value of positions in USD
    optVal: str  # Option value, only applicable to OPTION
    pendingCloseOrdLiabVal: str  # amount of close orders of isolated margin liability
    adl: str  # auto-deleveraging indicator, 1-5 in increasing risk of adl
    bizRefId: str  # external business id, eg experience coupon id
    bizRefType: str  # external business type
    ccy: str  # currency used for margin
    last: str  # last traded price
    idxPx: str  # last underlying index price
    usdPx: str  # last USD price of the ccy on the market, only applicable to OPTION
    bePx: str  # breakeven price
    deltaBS: str  # black-scholes delta in USD, only applicable to OPTION
    deltaPA: str  # black-scholes delta in coins, only applicable to OPTION
    gammaBS: str  # black-scholes gamma in USD, only applicable to OPTION
    gammaPA: str  # black-scholes gamma in coins, only applicable to OPTION
    thetaBS: str  # black-scholes theta in USD, only applicable to OPTION
    thetaPA: str  # black-scholes theta in coins, only applicable to OPTION
    vegaBS: str  # black-scholes vega in USD, only applicable to OPTION
    vegaPA: str  # black-scholes vega in coins, only applicable to OPTION
    spotInUseAmt: str  # spot in use amount, applicable to portfolio margin
    spotInUseCcy: str  # spot in use unit, eg BTC, applicable to portfolio margin
    clSpotInUseAmt: str  # user-defined spot risk offset amount, applicable to portfolio margin
    maxSpotInUseAmt: str  # max possible spot risk offset amount, applicable to portfolio margin
    realizedPnl: str  # realized pnl, applicable to FUTURES/SWAP/OPTION, pnll+fee+fundingFee+liqPen
    pnl: str  # accumulated pnl of closing order(s)
    fee: str  # accumulated fee, negative means user tx fee charged by platform, positive is rebate
    fundingFee: str  # accumulated funding fee
    liqPenalty: str  # accumulated liquidation penalty, negative when present
    closeOrderAlgo: list[OKXCloseOrderAlgoData]
    cTime: str
    uTime: str
    pTime: str  # push time of positions

    def parse_to_position_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        ts_init: int,
    ) -> PositionStatusReport:
        position_side = self.posSide.parse_to_position_side(self.pos)
        size = Quantity.from_str(self.pos.removeprefix("-"))  # Quantity does not accept negatives
        return PositionStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            position_side=position_side,
            quantity=size,
            report_id=report_id,
            ts_init=ts_init,
            ts_last=millis_to_nanos(Decimal(self.uTime)),
        )


class OKXWsPositionsArg(msgspec.Struct):
    channel: str
    uid: str
    instType: str
    instFamily: str
    instId: str


class OKXWsPositionsPushDataMsg(msgspec.Struct):
    arg: OKXWsPositionsArg
    data: list[OKXWsPositionsData]


def decoder_ws_positions() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsPositionsPushDataMsg)


################################################################################
# Private - Fills
################################################################################


class OKXWsFillsData(msgspec.Struct):
    instId: str
    fillSz: str
    fillPx: str
    side: OKXOrderSide
    ts: str
    ordId: str
    tradeId: str
    execType: OKXExecutionType
    count: str

    def parse_to_fill_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        report_id: UUID4,
        enum_parser: OKXEnumParser,
        ts_init: int,
        client_order_id: ClientOrderId,
        commission: Money,
    ) -> FillReport:
        return FillReport(
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(str(self.ordId)),
            trade_id=TradeId(self.tradeId),
            account_id=account_id,
            instrument_id=instrument_id,
            order_side=enum_parser.parse_okx_order_side(self.side),
            last_qty=Quantity.from_str(self.fillSz),
            last_px=Price.from_str(self.fillPx),
            commission=commission,
            liquidity_side=self.execType.parse_to_liquidity_side(),
            report_id=report_id,
            ts_event=millis_to_nanos(Decimal(self.ts)),
            ts_init=ts_init,
        )


class OKXWsFillsArg(msgspec.Struct):
    channel: str
    instId: str | None = None


class OKXWsFillsPushDataMsg(msgspec.Struct):
    arg: OKXWsFillsArg
    data: list[OKXWsFillsData]


def decoder_ws_fills() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(OKXWsFillsPushDataMsg)


# TODO
################################################################################
# Business - Tickers
################################################################################

# TODO
################################################################################
# Business - Candlesticks
################################################################################
