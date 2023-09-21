from typing import Optional

import msgspec
from pandas.core.common import maybe_iterable_to_list

from nautilus_trader.adapters.bybit.schemas.common import BybitListResult
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BarType, Bar
from nautilus_trader.model.objects import Price, Quantity


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

    def parse_to_bar(
        self,
        bar_type: BarType,
        ts_init: int,
    )-> Bar:
        return Bar(
            bar_type=bar_type,
            open=Price.from_str(self.openPrice),
            high=Price.from_str(self.highPrice),
            low=Price.from_str(self.lowPrice),
            close=Price.from_str(self.closePrice),
            volume=Quantity.from_str(self.volume),
            ts_event= millis_to_nanos(int(self.startTime)),
            ts_init=ts_init
        )

class BybitKlinesList(msgspec.Struct):
    symbol: str
    category: str
    list: list[BybitKline]

class BybitKlinesResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitKlinesList
    time: int

