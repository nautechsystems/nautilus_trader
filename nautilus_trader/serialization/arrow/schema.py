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

from nautilus_trader.model.data.tick import TradeTick
from nautilus_trader.model.instruments.betting import BettingInstrument
from nautilus_trader.model.orderbook.data import OrderBookData
from nautilus_trader.model.orderbook.data import OrderBookDelta


TYPE_TO_SCHEMA = {}


TYPE_TO_SCHEMA[TradeTick] = pa.schema(
    {
        "instrument_id": pa.string(),
        "price": pa.string(),
        "size": pa.string(),
        "aggressor_side": pa.string(),
        "match_id": pa.string(),
        "ts_event_ns": pa.int64(),
        "ts_recv_ns": pa.int64(),
    },
    metadata={"type": "TradeTick"},
)


TYPE_TO_SCHEMA[BettingInstrument] = pa.schema(
    {
        "venue": pa.string(),
        "currency": pa.string(),
        "instrument_id": pa.string(),
        "event_type_id": pa.string(),
        "event_type_name": pa.string(),
        "competition_id": pa.string(),
        "competition_name": pa.string(),
        "event_id": pa.string(),
        "event_name": pa.string(),
        "event_country_code": pa.string(),
        "event_open_date": pa.date64(),
        "betting_type": pa.string(),
        "market_id": pa.string(),
        "market_name": pa.string(),
        "market_start_time": pa.date64(),
        "market_type": pa.string(),
        "selection_id": pa.string(),
        "selection_name": pa.string(),
        "selection_handicap": pa.string(),
        "ts_recv_ns": pa.int64(),
        "ts_event_ns": pa.int64(),
    },
    metadata={"type": "BettingInstrument"},
)


TYPE_TO_SCHEMA[OrderBookData] = pa.schema(
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
    metadata={"type": "OrderBookData"},
)


TYPE_TO_SCHEMA[OrderBookDelta] = pa.schema(
    {
        "instrument_id": pa.string(),
        "ts_event_ns": pa.uint64(),
        "ts_recv_ns": pa.uint64(),
        "type": pa.string(),
        "side": pa.string(),
        "price": pa.float64(),
        "size": pa.float64(),
    },
    metadata={"type": "OrderBookDelta"},
)


SCHEMA_TO_TYPE = {v.metadata[b"type"]: k for k, v in TYPE_TO_SCHEMA.items()}


def register_schema(cls: type, schema: pa.Schema):
    global TYPE_TO_SCHEMA
    assert isinstance(cls, type)
    assert isinstance(schema, pa.Schema)
    if not schema.metadata:
        schema.add_metadata({"type": cls.__name__})
    TYPE_TO_SCHEMA[cls] = schema
    SCHEMA_TO_TYPE[schema.metadata[b"type"]] = cls


for x in OrderBookData.__subclasses__():
    register_schema(x, TYPE_TO_SCHEMA[OrderBookData])
