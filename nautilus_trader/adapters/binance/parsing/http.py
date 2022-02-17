# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

from typing import Dict, List

from nautilus_trader.adapters.binance.data_types import BinanceBar
from nautilus_trader.adapters.binance.parsing.common import parse_balances
from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data.bar import BarType
from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def parse_trade_tick_http(instrument_id: InstrumentId, msg: Dict, ts_init: int) -> TradeTick:
    return TradeTick(
        instrument_id=instrument_id,
        price=Price.from_str(msg["price"]),
        size=Quantity.from_str(msg["qty"]),
        aggressor_side=AggressorSide.SELL if msg["isBuyerMaker"] else AggressorSide.BUY,
        trade_id=TradeId(str(msg["id"])),
        ts_event=millis_to_nanos(msg["time"]),
        ts_init=ts_init,
    )


def parse_bar_http(bar_type: BarType, values: List, ts_init: int) -> BinanceBar:
    return BinanceBar(
        bar_type=bar_type,
        open=Price.from_str(values[1]),
        high=Price.from_str(values[2]),
        low=Price.from_str(values[3]),
        close=Price.from_str(values[4]),
        volume=Quantity.from_str(values[5]),
        quote_volume=Quantity.from_str(values[7]),
        count=values[8],
        taker_buy_base_volume=Quantity.from_str(values[9]),
        taker_buy_quote_volume=Quantity.from_str(values[10]),
        ts_event=millis_to_nanos(values[0]),
        ts_init=ts_init,
    )


def parse_account_balances_http(raw_balances: List[Dict[str, str]]) -> List[AccountBalance]:
    return parse_balances(raw_balances, "asset", "free", "locked")
