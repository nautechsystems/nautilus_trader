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

from libc.stdint cimport int64_t

import orjson
import pyarrow as pa

from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.serialization.base import register_serializable_object

from nautilus_trader.core.data cimport Data
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.data cimport OrderBookDelta


cpdef enum SubscriptionStatus:
    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


class InstrumentSearch(Data):
    def __init__(
        self,
        instruments,
        ts_event,
        ts_init,
    ):
        super().__init__(ts_event, ts_init)
        self.instruments = instruments


cdef class BSPOrderBookDelta(OrderBookDelta):
    @staticmethod
    def from_dict(dict values) -> BSPOrderBookDelta:
        return BSPOrderBookDelta.from_dict_c(values)

    @staticmethod
    def to_dict(BSPOrderBookDelta obj):
        return BSPOrderBookDelta.to_dict_c(obj)


cdef class BetfairTicker(Ticker):
    def __init__(
        self,
        InstrumentId instrument_id not None,
        int64_t ts_event,
        int64_t ts_init,
        Price last_traded_price=None,
        Quantity traded_volume=None,
        dict info=None,
    ):
        super().__init__(instrument_id=instrument_id, ts_event=ts_event, ts_init=ts_init, info=info)
        self.last_traded_price = last_traded_price
        self.traded_volume = traded_volume

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.dictionary(pa.int8(), pa.string()),
                "ts_event": pa.int64(),
                "ts_init": pa.int64(),
                "last_traded_price": pa.string(),
                "traded_volume": pa.string(),
            },
            metadata={"type": "BetfairTicker"}
        )


def betfair_ticker_from_dict(values: dict):
    return BetfairTicker(
        instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
        ts_event=values["ts_event"],
        ts_init=values["ts_init"],
        last_traded_price=Price.from_str_c(values["last_traded_price"]) if values['last_traded_price'] else None,
        traded_volume=Quantity.from_str_c(values["traded_volume"]) if values['traded_volume'] else None,
        info=orjson.loads(values['info']) if values.get('info') is not None else None,
    )


def betfair_ticker_to_dict(ticker: BetfairTicker):
    return {
        "type": type(ticker).__name__,
        "instrument_id": ticker.instrument_id.value,
        "ts_event": ticker.ts_event,
        "ts_init": ticker.ts_init,
        "last_traded_price": str(ticker.last_traded_price) if ticker.last_traded_price else None,
        "traded_volume": str(ticker.traded_volume) if ticker.traded_volume else None,
        "info": orjson.dumps(ticker.info) if ticker.info is not None else None,
    }


BSP_SCHEMA =pa.schema(
    {
        "instrument_id": pa.string(),
        "ts_event": pa.int64(),
        "ts_init": pa.int64(),
        "delta_type": pa.string(),
        "order_side": pa.string(),
        "order_price": pa.float64(),
        "order_size": pa.float64(),
        "order_id": pa.string(),
        "level": pa.string(),
    },
    metadata={"type": "BSPOrderBookDelta"},
)


register_serializable_object(BetfairTicker, betfair_ticker_to_dict, betfair_ticker_from_dict)
register_parquet(cls=BetfairTicker, schema=BetfairTicker.schema())

register_serializable_object(BSPOrderBookDelta, BSPOrderBookDelta.to_dict, BSPOrderBookDelta.from_dict)
register_parquet(cls=BSPOrderBookDelta, schema=BSP_SCHEMA)
