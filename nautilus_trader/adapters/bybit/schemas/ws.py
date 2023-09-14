from typing import Optional

import msgspec

from nautilus_trader.core.datetime import millis_to_nanos

from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
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
