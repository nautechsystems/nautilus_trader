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

from typing import Any

import msgspec

from nautilus_trader.adapters.bybit.common.parsing import parse_aggressor_side
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitTrade(msgspec.Struct):
    """https://bybit-exchange.github.io/docs/v5/market/recent-trade"""

    execId: str
    symbol: str
    price: str
    size: str
    side: str  # Side of taker (aggressor)
    time: str  # UNIX milliseconds
    isBlockTrade: bool
    mP: str | None = None  # (Options only)
    iP: str | None = None  # (Options only)
    mlv: str | None = None  # (Options only)
    iv: str | None = None  # (Options only)

    def parse_to_trade(
        self,
        instrument_id: InstrumentId,
        ts_init: int | None = None,
    ) -> TradeTick:
        ts_event = millis_to_nanos(int(self.time))

        return TradeTick(
            instrument_id=instrument_id,
            price=Price.from_str(self.price),
            size=Quantity.from_str(self.size),
            aggressor_side=parse_aggressor_side(self.side),
            trade_id=TradeId(self.execId),
            ts_event=ts_event,
            ts_init=(ts_init or ts_event),
        )


class BybitTradesList(msgspec.Struct):
    category: str
    list: list[BybitTrade]


class BybitTradesResponse(msgspec.Struct):
    retCode: int
    retMsg: str
    result: BybitTradesList
    retExtInfo: dict[str, Any]
    time: int
