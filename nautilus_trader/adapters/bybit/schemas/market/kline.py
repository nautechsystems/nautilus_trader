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

import datetime as dt

import msgspec

from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitKline(msgspec.Struct, array_like=True):
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
        timestamp_on_close: bool,
        ts_init: int | None = None,
    ) -> Bar:
        ts_event = millis_to_nanos(int(self.startTime))

        if timestamp_on_close:
            interval_ms = bar_type.spec.timedelta / dt.timedelta(milliseconds=1)
            ts_event += millis_to_nanos(interval_ms)

        return Bar(
            bar_type=bar_type,
            open=Price.from_str(self.openPrice),
            high=Price.from_str(self.highPrice),
            low=Price.from_str(self.lowPrice),
            close=Price.from_str(self.closePrice),
            volume=Quantity.from_str(self.volume),
            ts_event=ts_event,
            ts_init=(ts_init or ts_event),
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
