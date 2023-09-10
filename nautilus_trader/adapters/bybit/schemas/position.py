import msgspec

from nautilus_trader.adapters.bybit.schemas.common import BybitListResult


class BybitPositionStruct(msgspec.Struct):
    positionIdx: int
    riskId: int
    riskLimitValue: str
    symbol: str
    side: str
    size: str
    avgPrice: str
    positionValue: str
    tradeMode: int
    positionStatus: str
    autoAddMargin: int
    adlRankIndicator: int
    leverage: str
    positionBalance: str
    markPrice: str
    liqPrice: str
    bustPrice: str
    positionMM: str
    positionIM: str
    tpslMode: str
    takeProfit: str
    stopLoss: str
    trailingStop: str
    unrealisedPnl: str
    cumRealisedPnl: str
    createdTime: str
    updatedTime: str


class BybitPositionResponseStruct(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitListResult(BybitPositionStruct)
    time: int
