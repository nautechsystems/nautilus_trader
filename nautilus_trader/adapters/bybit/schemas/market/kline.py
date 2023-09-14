from typing import Optional

import msgspec

from nautilus_trader.adapters.bybit.schemas.common import BybitListResult


class BybitKline(msgspec.Struct,array_like=True):
    startTime: str
    openPrice: str
    highPrice: str
    lowPrice: str
    closePrice: str
    # Trade volume. Unit of contract:
    # pieces of contract. Unit of spot: quantity of coins
    volume: str
    # Turnover. Unit of figure: quantity of quota coin
    turnover: str

class BybitKlinesList(msgspec.Struct):
    symbol: str
    category: str
    list: list[BybitKline]

class BybitKlinesResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitKlinesList
    time: int

