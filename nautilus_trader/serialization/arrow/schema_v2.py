# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick


NAUTILUS_PARQUET_SCHEMA_V2 = {
    OrderBookDelta: pa.schema(
        {
            "action": pa.uint8(),
            "side": pa.uint8(),
            "price": pa.int64(),
            "size": pa.uint64(),
            "order_id": pa.uint64(),
            "flags": pa.uint8(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={
            "type": "OrderBookDelta",
            "book_type": ...,
            "instrument_id": ...,
            "price_precision": ...,
            "size_precision": ...,
        },
    ),
    QuoteTick: pa.schema(
        {
            "bid_price": pa.int64(),
            "bid_size": pa.uint64(),
            "ask_price": pa.int64(),
            "ask_size": pa.uint64(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={
            "type": "QuoteTick",
            "instrument_id": ...,
            "price_precision": ...,
            "size_precision": ...,
        },
    ),
    TradeTick: pa.schema(
        {
            "price": pa.int64(),
            "size": pa.uint64(),
            "aggressor_side": pa.int8(),
            "trade_id": pa.string(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={
            "type": "TradeTick",
            "instrument_id": ...,
            "price_precision": ...,
            "size_precision": ...,
        },
    ),
    Bar: pa.schema(
        {
            "open": pa.int64(),
            "high": pa.int64(),
            "low": pa.int64(),
            "close": pa.int64(),
            "volume": pa.uint64(),
            "ts_event": pa.uint64(),
            "ts_init": pa.uint64(),
        },
        metadata={
            "type": "Bar",
            "bar_type": ...,
            "instrument_id": ...,
            "price_precision": ...,
            "size_precision": ...,
        },
    ),
}
