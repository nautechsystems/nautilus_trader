from typing import Optional

import msgspec
from betfair_parser.spec.common import OrderStatus

from nautilus_trader.core.datetime import millis_to_nanos

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType, BybitEnumParser, BybitOrderType, \
    BybitOrderSide, BybitTimeInForce, BybitOrderStatus
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId, ClientOrderId, VenueOrderId, AccountId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


################################################################################
# Trade
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
    data: list[BybitWsTrade]

def decoder_ws_trade():
    return msgspec.json.Decoder(BybitWsTradeMsg)


################################################################################
# Ticker
################################################################################

class BybitWsTickerLinear(msgspec.Struct,omit_defaults=True,kw_only=True):
    symbol: str
    tickDirection: Optional[str] = None
    price24hPcnt: str
    lastPrice: Optional[str] = None
    prevPrice24h: Optional[str] = None
    highPrice24h: Optional[str] = None
    lowPrice24h: Optional[str] = None
    prevPrice1h: Optional[str] = None
    markPrice: Optional[str] = None
    indexPrice: Optional[str] = None
    openInterest: Optional[str] = None
    openInterestValue: Optional[str] = None
    turnover24h: Optional[str] = None
    volume24h: Optional[str] = None
    nextFundingTime: Optional[str] = None
    fundingRate: str
    bid1Price: str
    bid1Size: str
    ask1Price: str
    ask1Size: str


class BybitWsTickerLinearMsg(msgspec.Struct):
    topic: str
    type: str
    data: BybitWsTickerLinear


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
    data: BybitWsTickerSpot


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
    data: BybitWsTickerOption


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
# Account Order Update
################################################################################

class BybitWsAccountOrderUpdate(msgspec.Struct):
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
        enum_parser: BybitEnumParser
    )-> OrderStatusReport:
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
            ts_init=ts_init

        )


class BybitWsAccountOrderUpdateMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountOrderUpdate]


################################################################################
# Account Execution Update
################################################################################


class BybitWsAccountExecutionUpdate(msgspec.Struct):
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
    orderType: str
    stopOrderType: str
    side: str
    execTime: str
    isLeverage: str
    closedSize: str


class BybitWsAccountExecutionUpdateMsg(msgspec.Struct):
    topic: str
    id: str
    creationTime: int
    data: list[BybitWsAccountExecutionUpdate]