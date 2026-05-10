# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarAggregation
from nautilus_trader.model import BarSpecification
from nautilus_trader.model import BarType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick


class TestDataProviderPyo3:
    @staticmethod
    def quote_tick(
        instrument_id=None,
        bid_price=1987.0,
        ask_price=1988.0,
        ask_size=100_000.0,
        bid_size=100_000.0,
        ts_event=0,
        ts_init=0,
    ):
        return QuoteTick(
            instrument_id=instrument_id or InstrumentId.from_str("ETHUSDT.BINANCE"),
            bid_price=Price.from_str(str(bid_price)),
            ask_price=Price.from_str(str(ask_price)),
            bid_size=Quantity.from_str(str(bid_size)),
            ask_size=Quantity.from_str(str(ask_size)),
            ts_event=ts_event,
            ts_init=ts_init,
        )

    @staticmethod
    def trade_tick(
        instrument_id=None,
        price=1987.0,
        size=0.1,
        ts_event=0,
        ts_init=0,
    ):
        return TradeTick(
            instrument_id=instrument_id or InstrumentId.from_str("ETHUSDT.BINANCE"),
            price=Price.from_str(str(price)),
            size=Quantity.from_str(str(size)),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("1"),
            ts_init=ts_init,
            ts_event=ts_event,
        )

    @staticmethod
    def bar_5decimal():
        bar_type = BarType(
            InstrumentId.from_str("ETHUSDT.BINANCE"),
            BarSpecification(1, BarAggregation.MINUTE, PriceType.BID),
        )
        return Bar(
            bar_type=bar_type,
            open=Price.from_str("1.00002"),
            high=Price.from_str("1.00004"),
            low=Price.from_str("1.00001"),
            close=Price.from_str("1.00003"),
            volume=Quantity.from_int(1_000_000),
            ts_event=0,
            ts_init=0,
        )
