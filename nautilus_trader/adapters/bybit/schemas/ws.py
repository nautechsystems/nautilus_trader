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

import msgspec

from nautilus_trader.adapters.bybit.common.enums import BybitEnumParser
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.bybit.common.enums import BybitKlineInterval
from nautilus_trader.adapters.bybit.common.enums import BybitOrderSide
from nautilus_trader.adapters.bybit.common.enums import BybitOrderStatus
from nautilus_trader.adapters.bybit.common.enums import BybitOrderType
from nautilus_trader.adapters.bybit.common.enums import BybitPositionIdx
from nautilus_trader.adapters.bybit.common.enums import BybitTimeInForce
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitWsMessageGeneral(msgspec.Struct):
    topic: str | None = None
    success: bool | None = None
    ret_msg: str | None = None
    op: str | None = None


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
# Public - Orderbook Delta
################################################################################


class BybitWsOrderbookDeltaData(msgspec.Struct):
    # symbol
    s: str
    # bids
    b: list[list[str]]
    # asks
    a: list[list[str]]


class BybitWsOrderbookDeltaMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    data: BybitWsOrderbookDeltaData


################################################################################
# Public - Orderbook Snapshot
################################################################################


class BybitWsOrderbookSnapshot(msgspec.Struct):
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


class BybitWsOrderbookSnapshotMsg(msgspec.Struct):
    topic: str
    type: str
    ts: int
    data: BybitWsOrderbookSnapshot


################################################################################
# Public - Ticker Linear
################################################################################


class BybitWsTickerLinear(msgspec.Struct, omit_defaults=True, kw_only=True):
    symbol: str
    tickDirection: str | None = None
    price24hPcnt: str
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
    fundingRate: str
    bid1Price: str
    bid1Size: str
    ask1Price: str
    ask1Size: str


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
    # Direction of price change
    L: str
    # Trade id
    i: str
    # Whether is a block trade or not
    BT: bool

    def parse_to_trade_tick(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> TradeTick:
        return TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(self.p),
            size=Quantity.from_str(self.v),
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


def decoder_ws_trade():
    return msgspec.json.Decoder(BybitWsTradeMsg)


def decoder_ws_ticker(instrument_type: BybitInstrumentType):
    if instrument_type == BybitInstrumentType.LINEAR:
        return msgspec.json.Decoder(BybitWsTickerLinearMsg)
    elif instrument_type == BybitInstrumentType.SPOT:
        return msgspec.json.Decoder(BybitWsTickerSpotMsg)
    elif instrument_type == BybitInstrumentType.OPTION:
        return msgspec.json.Decoder(BybitWsTickerOptionMsg)
    else:
        raise ValueError(f"Invalid account type: {instrument_type}")


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
    unrealisedPnl: str
    cumRealisedPnl: str
    createdTime: str
    updatedTime: str
    tpslMode: str
    liqPrice: str
    bustPrice: str
    category: str
    positionStatus: str
    adlRankIndicator: int
    seq: int


class BybitWsAccountPositionMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountPosition]


################################################################################
# Private - Account Order
################################################################################


class BybitWsAccountOrder(msgspec.Struct):
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
    stopOrderType: str
    tpslMode: str
    triggerPrice: str
    takeProfit: str
    stopLoss: str
    tpTriggerBy: str
    slTriggerBy: str
    tpLimitPrice: str
    slLimitPrice: str
    triggerDirection: int
    triggerBy: str
    closeOnTrigger: bool
    category: str
    placeType: str
    smpType: str
    smpGroup: int
    smpOrderId: str
    feeCurrency: str

    def parse_to_order_status_report(
        self,
        account_id: AccountId,
        instrument_id: InstrumentId,
        enum_parser: BybitEnumParser,
    ) -> OrderStatusReport:
        client_order_id = ClientOrderId(str(self.orderLinkId))
        price = Price.from_str(self.price) if self.price else None
        ts_event = millis_to_nanos(int(self.updatedTime))
        venue_order_id = VenueOrderId(str(self.orderId))
        ts_init = millis_to_nanos(int(self.createdTime))

        return OrderStatusReport(
            account_id=account_id,
            instrument_id=instrument_id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            order_side=enum_parser.parse_bybit_order_side(self.side),
            order_type=enum_parser.parse_bybit_order_type(self.orderType),
            time_in_force=enum_parser.parse_bybit_time_in_force(self.timeInForce),
            order_status=enum_parser.parse_bybit_order_status(self.orderStatus),
            price=price,
            quantity=Quantity.from_str(self.qty),
            filled_qty=Quantity.from_str(self.cumExecQty),
            report_id=UUID4(),
            ts_accepted=ts_event,
            ts_last=ts_event,
            ts_init=ts_init,
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
    category: str
    symbol: str
    execFee: str
    execId: str
    execPrice: str
    execQty: str
    execType: str
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
    stopOrderType: str
    side: BybitOrderSide
    execTime: str
    isLeverage: str
    closedSize: str
    seq: int


class BybitWsAccountExecutionMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountExecution]


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


class BybitWsAccountWalletMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountWallet]
