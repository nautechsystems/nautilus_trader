# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pyarrow as pa

from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.orderbook.book import OrderBookData
from nautilus_trader.model.tick import QuoteTick
from nautilus_trader.model.tick import TradeTick


NAUTILUS_PARQUET_SCHEMA = {
    TradeTick: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "price": pa.string(),
            "size": pa.string(),
            "aggressor_side": pa.dictionary(pa.int8(), pa.string()),
            "match_id": pa.string(),
            "ts_event_ns": pa.int64(),
            "ts_recv_ns": pa.int64(),
        },
        metadata={"type": "TradeTick"},
    ),
    QuoteTick: pa.schema(
        {
            "instrument_id": pa.dictionary(pa.int8(), pa.string()),
            "bid": pa.string(),
            "bid_size": pa.string(),
            "ask": pa.string(),
            "ask_size": pa.string(),
            "ts_event_ns": pa.int64(),
            "ts_recv_ns": pa.int64(),
        },
        metadata={"type": "QuoteTick"},
    ),
    BettingInstrument: pa.schema(
        {
            "venue_name": pa.string(),
            "currency": pa.string(),
            "instrument_id": pa.string(),
            "event_type_id": pa.string(),
            "event_type_name": pa.string(),
            "competition_id": pa.string(),
            "competition_name": pa.string(),
            "event_id": pa.string(),
            "event_name": pa.string(),
            "event_country_code": pa.string(),
            "event_open_date": pa.string(),
            "betting_type": pa.string(),
            "market_id": pa.string(),
            "market_name": pa.string(),
            "market_start_time": pa.string(),
            "market_type": pa.string(),
            "selection_id": pa.string(),
            "selection_name": pa.string(),
            "selection_handicap": pa.string(),
            "ts_recv_ns": pa.int64(),
            "ts_event_ns": pa.int64(),
        },
        metadata={"type": "BettingInstrument"},
    ),
    OrderBookData: pa.schema(
        {
            "instrument_id": pa.string(),
            "ts_event_ns": pa.uint64(),
            "ts_recv_ns": pa.uint64(),
            "delta_type": pa.string(),
            "order_side": pa.string(),
            "order_price": pa.float64(),
            "order_size": pa.float64(),
            "order_id": pa.string(),
            "level": pa.string(),
        },
        metadata={"type": "OrderBookDelta"},
    ),
}

# SCHEMA_TO_TYPE = {v.metadata[b"type"]: k for k, v in TYPE_TO_SCHEMA.items()}
