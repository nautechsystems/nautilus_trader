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
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Price
from nautilus_trader.model import PriceType
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import Symbol
from nautilus_trader.model import TradeId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.serialization import bars_to_arrow_record_batch_bytes
from nautilus_trader.serialization import get_arrow_schema_map
from nautilus_trader.serialization import index_prices_to_arrow_record_batch_bytes
from nautilus_trader.serialization import instrument_closes_to_arrow_record_batch_bytes
from nautilus_trader.serialization import mark_prices_to_arrow_record_batch_bytes
from nautilus_trader.serialization import quotes_to_arrow_record_batch_bytes
from nautilus_trader.serialization import trades_to_arrow_record_batch_bytes


INSTRUMENT_ID = InstrumentId(Symbol("AUD/USD"), Venue("SIM"))


def test_get_arrow_schema_map_quote_tick():
    schema = get_arrow_schema_map(QuoteTick)

    assert isinstance(schema, dict)
    assert len(schema) > 0


def test_get_arrow_schema_map_trade_tick():
    schema = get_arrow_schema_map(TradeTick)

    assert isinstance(schema, dict)
    assert len(schema) > 0


def test_get_arrow_schema_map_bar():
    schema = get_arrow_schema_map(Bar)

    assert isinstance(schema, dict)
    assert len(schema) > 0


def test_quotes_to_arrow_record_batch_bytes():
    quotes = [
        QuoteTick(
            instrument_id=INSTRUMENT_ID,
            bid_price=Price.from_str("0.80000"),
            ask_price=Price.from_str("0.80010"),
            bid_size=Quantity.from_int(1_000_000),
            ask_size=Quantity.from_int(1_000_000),
            ts_event=1,
            ts_init=2,
        ),
        QuoteTick(
            instrument_id=INSTRUMENT_ID,
            bid_price=Price.from_str("0.80005"),
            ask_price=Price.from_str("0.80015"),
            bid_size=Quantity.from_int(500_000),
            ask_size=Quantity.from_int(500_000),
            ts_event=3,
            ts_init=4,
        ),
    ]

    result = quotes_to_arrow_record_batch_bytes(quotes)

    assert isinstance(result, bytes)
    assert len(result) > 0


def test_trades_to_arrow_record_batch_bytes():
    trades = [
        TradeTick(
            instrument_id=INSTRUMENT_ID,
            price=Price.from_str("0.80000"),
            size=Quantity.from_int(100_000),
            aggressor_side=AggressorSide.BUYER,
            trade_id=TradeId("T-001"),
            ts_event=1,
            ts_init=2,
        ),
    ]

    result = trades_to_arrow_record_batch_bytes(trades)

    assert isinstance(result, bytes)
    assert len(result) > 0


def test_bars_to_arrow_record_batch_bytes():
    bar_type = BarType(
        instrument_id=INSTRUMENT_ID,
        spec=BarSpecification(
            step=1,
            aggregation=BarAggregation.MINUTE,
            price_type=PriceType.LAST,
        ),
    )
    bars = [
        Bar(
            bar_type=bar_type,
            open=Price.from_str("0.80000"),
            high=Price.from_str("0.80050"),
            low=Price.from_str("0.79950"),
            close=Price.from_str("0.80010"),
            volume=Quantity.from_int(1_000_000),
            ts_event=1,
            ts_init=2,
        ),
    ]

    result = bars_to_arrow_record_batch_bytes(bars)

    assert isinstance(result, bytes)
    assert len(result) > 0


def test_mark_prices_to_arrow_record_batch_bytes():
    marks = [
        MarkPriceUpdate(
            instrument_id=INSTRUMENT_ID,
            value=Price.from_str("0.80000"),
            ts_event=1,
            ts_init=2,
        ),
    ]

    result = mark_prices_to_arrow_record_batch_bytes(marks)

    assert isinstance(result, bytes)
    assert len(result) > 0


def test_index_prices_to_arrow_record_batch_bytes():
    prices = [
        IndexPriceUpdate(
            instrument_id=INSTRUMENT_ID,
            value=Price.from_str("0.80000"),
            ts_event=1,
            ts_init=2,
        ),
    ]

    result = index_prices_to_arrow_record_batch_bytes(prices)

    assert isinstance(result, bytes)
    assert len(result) > 0


def test_instrument_closes_to_arrow_record_batch_bytes():
    closes = [
        InstrumentClose(
            instrument_id=INSTRUMENT_ID,
            close_price=Price.from_str("0.80000"),
            close_type=InstrumentCloseType.END_OF_SESSION,
            ts_event=1,
            ts_init=2,
        ),
    ]

    result = instrument_closes_to_arrow_record_batch_bytes(closes)

    assert isinstance(result, bytes)
    assert len(result) > 0
