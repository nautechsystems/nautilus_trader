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

from __future__ import annotations

from collections.abc import Sequence
from decimal import Decimal
from typing import TYPE_CHECKING

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitExecType
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitPositionIdx
from nautilus_trader.adapters.bybit.common.enums import BybitProductType
from nautilus_trader.adapters.bybit.common.enums import BybitStopOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerDirection
from nautilus_trader.adapters.bybit.common.enums import BybitTriggerType
from nautilus_trader.adapters.bybit.common.enums import BybitWsOrderRequestMsgOP
from nautilus_trader.adapters.bybit.common.parsing import parse_bybit_delta
from nautilus_trader.adapters.bybit.endpoints.trade.amend_order import BybitAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_amend_order import BybitBatchAmendOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_cancel_order import BybitBatchCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.batch_place_order import BybitBatchPlaceOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.cancel_order import BybitCancelOrderPostParams
from nautilus_trader.adapters.bybit.endpoints.trade.place_order import BybitPlaceOrderPostParams
from nautilus_trader.adapters.bybit.schemas.order import BybitAmendOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchAmendOrderExtInfo
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchAmendOrderResult
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchCancelOrderExtInfo
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchCancelOrderResult
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchPlaceOrderExtInfo
from nautilus_trader.adapters.bybit.schemas.order import BybitBatchPlaceOrderResult
from nautilus_trader.adapters.bybit.schemas.order import BybitCancelOrder
from nautilus_trader.adapters.bybit.schemas.order import BybitPlaceOrder
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.enums import TrailingOffsetType
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


if TYPE_CHECKING:
    from nautilus_trader.adapters.bybit.execution import BybitExecutionClient


class BybitWsMessageGeneral(msgspec.Struct):
    op: str | None = None
    topic: str | None = None
    success: bool | None = None
    ret_msg: str | None = None


class BybitWsSubscriptionMsg(msgspec.Struct):
    success: bool
    op: str
    conn_id: str
    ret_msg: str | None = None
    req_id: str | None = None


class BybitWsPrivateChannelAuthMsg(msgspec.Struct, kw_only=True):
    success: bool
    ret_msg: str | None = None
    op: str
    conn_id: str

    def is_auth_success(self) -> bool:
        return (self.op == "auth") and (self.success is True)


class BybitWsTradeAuthMsg(msgspec.Struct, kw_only=True):
    reqId: str | None = None
    retCode: int
    retMsg: str
    op: str
    connId: str

    def is_auth_success(self) -> bool:
        return (self.op == "auth") and (self.retCode == 0)


################################################################################
# Public - Kline
################################################################################


class BybitWsKline(msgspec.Struct):
    start: int
    end: int
    interval: BybitKlineInterval
    open: str
    close: str
    high: str
    low: str
    volume: str
    turnover: str
    confirm: bool
    timestamp: int

    def parse_to_bar(
        self,
        bar_type: BarType,
        price_precision: int,
        size_precision: int,
        ts_init: int,
        timestamp_on_close: bool,
    ) -> Bar:
        if timestamp_on_close:
            ts_event = millis_to_nanos(self.end + 1)
        else:
            ts_event = millis_to_nanos(self.start)

        return Bar(
            bar_type=bar_type,
            open=Price(float(self.open), price_precision),
            high=Price(float(self.high), price_precision),
            low=Price(float(self.low), price_precision),
            close=Price(float(self.close), price_precision),
            volume=Quantity(float(self.volume), size_precision),
            ts_event=ts_event,
            ts_init=ts_init,
        )


class BybitWsKlineMsg(msgspec.Struct):
    # Topic name
    topic: str
    ts: int
    type: str
    data: list[BybitWsKline]


################################################################################
# Public - Liquidation
################################################################################


class BybitWsLiquidation(msgspec.Struct):
    price: str
    side: BybitOrderSide
    size: str
    symbol: str
    updatedTime: int


class BybitWsLiquidationMsg(msgspec.Struct):
    topic: str
    ts: int
    type: str
    data: BybitWsLiquidation


################################################################################
# Public - Orderbook depth
################################################################################


class BybitWsOrderbookDepth(msgspec.Struct):
    # symbol
    s: str
    # bids
    b: list[list[str]]
    # asks
    a: list[list[str]]
    # Update ID. Is a sequence. Occasionally, you'll receive "u"=1, which is a
    # snapshot data due to the restart of the service.
    u: int
    # Cross sequence
    seq: int

    def parse_to_deltas(
        self,
        instrument_id: InstrumentId,
        price_precision: int | None,
        size_precision: int | None,
        ts_event: int,
        ts_init: int,
        snapshot: bool = False,
    ) -> OrderBookDeltas:
        bids_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.b
        ]
        asks_raw = [
            (
                Price(float(d[0]), price_precision),
                Quantity(float(d[1]), size_precision),
            )
            for d in self.a
        ]

        deltas: list[OrderBookDelta] = []

        if snapshot:
            deltas.append(OrderBookDelta.clear(instrument_id, 0, ts_event, ts_init))

        bids_len = len(bids_raw)
        asks_len = len(asks_raw)

        for idx, bid in enumerate(bids_raw):
            flags = 0
            if idx == bids_len - 1 and asks_len == 0:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_bybit_delta(
                instrument_id=instrument_id,
                values=bid,
                side=OrderSide.BUY,
                update_id=self.u,
                flags=flags,
                sequence=self.seq,
                ts_event=ts_event,
                ts_init=ts_init,
                snapshot=snapshot,
            )
            deltas.append(delta)

        for idx, ask in enumerate(asks_raw):
            flags = 0
            if idx == asks_len - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_bybit_delta(
                instrument_id=instrument_id,
                values=ask,
                side=OrderSide.SELL,
                update_id=self.u,
                flags=flags,
                sequence=self.seq,
                ts_event=ts_event,
                ts_init=ts_init,
                snapshot=snapshot,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)

    def parse_to_quote_tick(
        self,
        instrument_id: InstrumentId,
        last_quote: QuoteTick,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> QuoteTick:
        top_bid = self.b[0] if self.b else None
        top_ask = self.a[0] if self.a else None
        top_bid_price = top_bid[0] if top_bid else None
        top_ask_price = top_ask[0] if top_ask else None
        top_bid_size = top_bid[1] if top_bid else None
        top_ask_size = top_ask[1] if top_ask else None

        if top_bid_size == "0":
            top_bid_size = None
        if top_ask_size == "0":
            top_ask_size = None

        # Convert the previous quote to new price and sizes to ensure that the precision
        # of the new Quote is consistent with the instrument definition even after
        # updates of the instrument.
        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=(
                Price(float(top_bid_price), price_precision)
                if top_bid_price
                else Price(last_quote.bid_price.as_double(), price_precision)
            ),
            ask_price=(
                Price(float(top_ask_price), price_precision)
                if top_ask_price
                else Price(last_quote.ask_price.as_double(), price_precision)
            ),
            bid_size=(
                Quantity(float(top_bid_size), size_precision)
                if top_bid_size
                else Quantity(last_quote.bid_size.as_double(), size_precision)
            ),
            ask_size=(
                Quantity(float(top_ask_size), size_precision)
                if top_ask_size
                else Quantity(last_quote.ask_size.as_double(), size_precision)
            ),
            ts_event=ts_event,
            ts_init=ts_init,
        )


class BybitWsOrderbookDepthMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    data: BybitWsOrderbookDepth


def decoder_ws_orderbook():
    return msgspec.json.Decoder(BybitWsOrderbookDepthMsg)


################################################################################
# Public - Ticker Linear
################################################################################


class BybitWsTickerLinear(msgspec.Struct, omit_defaults=True, kw_only=True):
    symbol: str
    tickDirection: str | None = None
    price24hPcnt: str | None = None
    lastPrice: str | None = None
    prevPrice24h: str | None = None
    highPrice24h: str | None = None
    lowPrice24h: str | None = None
    prevPrice1h: str | None = None
    markPrice: str | None = None
    indexPrice: str | None = None
    openInterest: str | None = None
    openInterestValue: str | None = None
    turnover24h: str | None = None
    volume24h: str | None = None
    nextFundingTime: str | None = None
    fundingRate: str | None = None
    bid1Price: str | None = None
    bid1Size: str | None = None
    ask1Price: str | None = None
    ask1Size: str | None = None

    def parse_to_quote_tick(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price.from_str(self.bid1Price),
            ask_price=Price.from_str(self.ask1Price),
            bid_size=Quantity.from_str(self.bid1Size),
            ask_size=Quantity.from_str(self.ask1Size),
            ts_event=ts_event,
            ts_init=ts_init,
        )


class BybitWsTickerLinearMsg(msgspec.Struct):
    topic: str
    type: str
    data: BybitWsTickerLinear
    cs: int
    ts: int


################################################################################
# Public - Ticker Spot
################################################################################


class BybitWsTickerSpot(msgspec.Struct):
    symbol: str
    lastPrice: str
    highPrice24h: str
    lowPrice24h: str
    prevPrice24h: str
    volume24h: str
    turnover24h: str
    price24hPcnt: str
    usdIndexPrice: str


class BybitWsTickerSpotMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    cs: int
    data: BybitWsTickerSpot


################################################################################
# Public - Ticker Option
################################################################################


class BybitWsTickerOption(msgspec.Struct):
    symbol: str
    bidPrice: str
    bidSize: str
    bidIv: str
    askPrice: str
    askSize: str
    askIv: str
    lastPrice: str
    highPrice24h: str
    lowPrice24h: str
    markPrice: str
    indexPrice: str
    markPriceIv: str
    underlyingPrice: str
    openInterest: str
    turnover24h: str
    volume24h: str
    totalVolume: str
    totalTurnover: str
    delta: str
    gamma: str
    vega: str
    theta: str
    predictedDeliveryPrice: str
    change24h: str

    def parse_to_quote_tick(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_event: int,
        ts_init: int,
    ) -> QuoteTick:
        return QuoteTick(
            instrument_id=instrument_id,
            bid_price=Price(float(self.bidPrice), price_precision),
            ask_price=Price(float(self.askPrice), price_precision),
            bid_size=Quantity(float(self.bidSize), size_precision),
            ask_size=Quantity(float(self.askSize), size_precision),
            ts_event=ts_event,
            ts_init=ts_init,
        )


class BybitWsTickerOptionMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    data: BybitWsTickerOption


################################################################################
# Public - Trade
################################################################################


class BybitWsTrade(msgspec.Struct):
    # The timestamp (ms) that the order is filled
    T: int
    # Symbol name
    s: str
    # Side of taker. Buy,Sell
    S: str
    # Trade size
    v: str
    # Trade price
    p: str
    # Trade id
    i: str
    # Whether is a block trade or not
    BT: bool
    # Direction of price change
    L: str | None = None
    # Message id unique to options
    id: str | None = None
    # Mark price, unique field for option
    mP: str | None = None
    # Index price, unique field for option
    iP: str | None = None
    # Mark iv, unique field for option
    mIv: str | None = None
    # iv, unique field for option
    iv: str | None = None

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        price_precision: int,
        size_precision: int,
        ts_init: int,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id,
            price=Price(float(self.p), price_precision),
            size=Quantity(float(self.v), size_precision),
            aggressor_side=AggressorSide.SELLER if self.S == "Sell" else AggressorSide.BUYER,
            trade_id=TradeId(str(self.i)),
            ts_event=millis_to_nanos(self.T),
            ts_init=ts_init,
        )


class BybitWsTradeMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    data: list[BybitWsTrade]


def decoder_ws_trade() -> msgspec.json.Decoder:
    return msgspec.json.Decoder(BybitWsTradeMsg)


def decoder_ws_kline():
    return msgspec.json.Decoder(BybitWsKlineMsg)


################################################################################
# Private - Account Position
################################################################################


class BybitWsAccountPosition(msgspec.Struct):
    positionIdx: BybitPositionIdx
    tradeMode: int
    riskId: int
    riskLimitValue: str
    symbol: str
    side: BybitOrderSide
    size: str
    entryPrice: str
    leverage: str
    positionValue: str
    positionBalance: str
    markPrice: str
    positionIM: str
    positionMM: str
    takeProfit: str
    stopLoss: str
    trailingStop: str
    sessionAvgPrice: str
    unrealisedPnl: str
    cumRealisedPnl: str
    createdTime: str
    updatedTime: str
    liqPrice: str
    bustPrice: str
    category: BybitProductType
    positionStatus: str
    adlRankIndicator: int
    seq: int
    autoAddMargin: int
    leverageSysUpdatedTime: str
    mmrSysUpdatedTime: str
    isReduceOnly: bool
    tpslMode: str | None = None


class BybitWsAccountPositionMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountPosition]


################################################################################
# Private - Account Order
################################################################################


class BybitWsAccountOrder(msgspec.Struct):
    category: BybitProductType
    symbol: str
    orderId: str
    side: BybitOrderSide
    orderType: BybitOrderType
    cancelType: str
    price: str
    qty: str
    orderIv: str
    timeInForce: BybitTimeInForce
    orderStatus: BybitOrderStatus
    orderLinkId: str
    lastPriceOnCreated: str
    reduceOnly: bool
    leavesQty: str
    leavesValue: str
    cumExecQty: str
    cumExecValue: str
    avgPrice: str
    blockTradeId: str
    positionIdx: int
    cumExecFee: str
    createdTime: str
    updatedTime: str
    rejectReason: str
    triggerPrice: str
    takeProfit: str
    stopLoss: str
    tpTriggerBy: str
    slTriggerBy: str
    tpLimitPrice: str
    slLimitPrice: str
    closeOnTrigger: bool
    placeType: str
    smpType: str
    smpGroup: int
    smpOrderId: str
    feeCurrency: str
    triggerBy: BybitTriggerType
    stopOrderType: BybitStopOrderType
    triggerDirection: BybitTriggerDirection = BybitTriggerDirection.NONE
    tpslMode: str | None = None
    createType: str | None = None

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        client_order_id: ClientOrderId,
        enum_parser: BybitEnumParser,
        ts_init: int,
    ) -> OrderStatusReport:
        order_type = enum_parser.parse_bybit_order_type(
            self.orderType,
            self.stopOrderType,
            self.side,
            self.triggerDirection,
        )
        trigger_price = Price.from_str(self.triggerPrice) if self.triggerPrice else None
        trigger_type = enum_parser.parse_bybit_trigger_type(self.triggerBy)

        if order_type in (OrderType.TRAILING_STOP_MARKET, OrderType.TRAILING_STOP_LIMIT):
            assert trigger_price is not None  # Type checking
            last_price = Decimal(self.lastPriceOnCreated)
            trailing_offset = abs(trigger_price.as_decimal() - last_price)
            trailing_offset_type = TrailingOffsetType.PRICE
        else:
            trailing_offset = None
            trailing_offset_type = TrailingOffsetType.NO_TRAILING_OFFSET

        order_status = enum_parser.parse_bybit_order_status(order_type, self.orderStatus)

        # Special case: if Bybit reports "Rejected" but the order has fills, treat it as Canceled.
        # This handles the case where the exchange partially fills an order then rejects the
        # remaining quantity (e.g., due to margin, risk limits, or liquidity constraints).
        # The state machine does not allow PARTIALLY_FILLED -> REJECTED transitions.
        if self.orderStatus == BybitOrderStatus.REJECTED and Decimal(self.cumExecQty) > 0:
            order_status = OrderStatus.CANCELED

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=VenueOrderId(self.orderId),
            order_side=enum_parser.parse_bybit_order_side(self.side),
            order_type=order_type,
            time_in_force=enum_parser.parse_bybit_time_in_force(self.timeInForce),
            order_status=order_status,
            price=Price.from_str(self.price) if self.price else None,
            trigger_price=trigger_price,
            trigger_type=trigger_type,
            trailing_offset=trailing_offset,
            trailing_offset_type=trailing_offset_type,
            quantity=Quantity.from_str(self.qty),
            filled_qty=Quantity.from_str(self.cumExecQty),
            report_id=UUID4(),
            ts_accepted=millis_to_nanos(int(self.createdTime)),
            ts_last=millis_to_nanos(int(self.updatedTime)),
            ts_init=ts_init,
            avg_px=Decimal(self.avgPrice) if self.avgPrice else None,
            reduce_only=self.reduceOnly,
            post_only=self.timeInForce == BybitTimeInForce.POST_ONLY.value,
        )


class BybitWsAccountOrderMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountOrder]


################################################################################
# Private - Account Execution
################################################################################


class BybitWsAccountExecution(msgspec.Struct):
    category: BybitProductType
    symbol: str
    execFee: str
    execId: str
    execPrice: str
    execQty: str
    execType: BybitExecType
    execValue: str
    isMaker: bool
    feeRate: str
    tradeIv: str
    markIv: str
    blockTradeId: str
    markPrice: str
    indexPrice: str
    underlyingPrice: str
    leavesQty: str
    orderId: str
    orderLinkId: str
    orderPrice: str
    orderQty: str
    orderType: BybitOrderType
    side: BybitOrderSide
    execTime: str
    isLeverage: str
    closedSize: str
    seq: int
    stopOrderType: BybitStopOrderType


class BybitWsAccountExecutionMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountExecution]


################################################################################
# Private - Account Execution Fast
################################################################################


class BybitWsAccountExecutionFast(msgspec.Struct):
    category: BybitProductType
    symbol: str
    orderId: str
    isMaker: bool
    orderLinkId: str
    side: BybitOrderSide
    execId: str
    execPrice: str
    execQty: str
    execTime: str
    seq: int

    orderType: BybitOrderType = BybitOrderType.UNKNOWN
    stopOrderType: BybitStopOrderType = BybitStopOrderType.NONE


class BybitWsAccountExecutionFastMsg(msgspec.Struct):
    topic: str
    creationTime: int
    data: list[BybitWsAccountExecutionFast]


################################################################################
# Private - Account Wallet
################################################################################


class BybitWsAccountWalletCoin(msgspec.Struct):
    coin: str
    equity: str
    usdValue: str
    walletBalance: str
    availableToWithdraw: str
    availableToBorrow: str
    borrowAmount: str
    accruedInterest: str
    totalOrderIM: str
    totalPositionIM: str
    totalPositionMM: str
    unrealisedPnl: str
    cumRealisedPnl: str
    bonus: str
    collateralSwitch: bool
    marginCollateral: bool
    locked: str
    spotHedgingQty: str
    spotBorrow: str | None = None

    def parse_to_account_balance(self) -> AccountBalance:
        currency = Currency.from_str(self.coin)
        wallet_balance = Decimal(self.walletBalance or "0")
        spot_borrow = Decimal(self.spotBorrow or "0")

        total_balance = wallet_balance - spot_borrow

        total = Money.from_str(f"{total_balance} {currency}")
        locked = Money.from_str(f"{self.locked or '0'} {currency}")
        free = Money.from_raw(total.raw - locked.raw, currency)

        return AccountBalance(
            total=total,
            locked=locked,
            free=free,
        )

    def parse_to_margin_balance(self) -> MarginBalance:
        currency = Currency.from_str(self.coin)

        return MarginBalance(
            initial=Money.from_str(f"{self.totalPositionIM or '0'} {currency}"),
            maintenance=Money.from_str(f"{self.totalPositionMM or '0'} {currency}"),
        )


class BybitWsAccountWallet(msgspec.Struct):
    accountIMRate: str
    accountMMRate: str
    totalEquity: str
    totalWalletBalance: str
    totalMarginBalance: str
    totalAvailableBalance: str
    totalPerpUPL: str
    totalInitialMargin: str
    totalMaintenanceMargin: str
    coin: list[BybitWsAccountWalletCoin]
    accountLTV: str
    accountType: str

    def parse_to_account_balance(self) -> list[AccountBalance]:
        return [coin.parse_to_account_balance() for coin in self.coin]

    def parse_to_margin_balance(self) -> list[MarginBalance]:
        return [coin.parse_to_margin_balance() for coin in self.coin]


class BybitWsAccountWalletMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountWallet]

    def handle_account_wallet_update(self, exec_client: BybitExecutionClient):
        for wallet in self.data:
            exec_client.generate_account_state(
                balances=wallet.parse_to_account_balance(),
                margins=wallet.parse_to_margin_balance(),
                reported=True,
                ts_event=millis_to_nanos(self.creationTime),
            )


################################################################################
# Trade
################################################################################


class BybitWsOrderRequestMsg(msgspec.Struct, kw_only=True):
    reqId: str | None = None
    header: dict[str, str]
    op: BybitWsOrderRequestMsgOP
    args: Sequence[
        BybitPlaceOrderPostParams
        | BybitAmendOrderPostParams
        | BybitCancelOrderPostParams
        | BybitBatchPlaceOrderPostParams
        | BybitBatchAmendOrderPostParams
        | BybitBatchCancelOrderPostParams
    ] = []


class BybitWsOrderResponseMsgGeneral(msgspec.Struct, kw_only=True):
    op: BybitWsOrderRequestMsgOP
    retCode: int
    reqId: str | None = None
    retMsg: str


class BybitWsOrderResponseMsg(BybitWsOrderResponseMsgGeneral, kw_only=True):
    header: dict[str, str] | None = None
    connId: str


class BybitWsPlaceOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitPlaceOrder


class BybitWsAmendOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitAmendOrder


class BybitWsCancelOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitCancelOrder


class BybitWsBatchPlaceOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitBatchPlaceOrderResult
    retExtInfo: BybitBatchPlaceOrderExtInfo


class BybitWsBatchAmendOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitBatchAmendOrderResult
    retExtInfo: BybitBatchAmendOrderExtInfo


class BybitWsBatchCancelOrderResponseMsg(BybitWsOrderResponseMsg):
    data: BybitBatchCancelOrderResult
    retExtInfo: BybitBatchCancelOrderExtInfo
