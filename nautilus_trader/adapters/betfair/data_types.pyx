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

import pyarrow as pa
from orjson import orjson

from nautilus_trader.serialization.arrow.serializer import register_parquet

from nautilus_trader.core.correctness cimport Condition
from nautilus_trader.model.data.base cimport Data
from nautilus_trader.model.data.ticker cimport Ticker
from nautilus_trader.model.identifiers cimport InstrumentId
from nautilus_trader.model.objects cimport Price
from nautilus_trader.model.objects cimport Quantity
from nautilus_trader.model.orderbook.data cimport OrderBookDelta

from nautilus_trader.serialization.base import register_serializable_object


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

    @staticmethod
    def from_dict(dict values) -> BetfairTicker:
        return BetfairTicker.from_dict_c(values)

    @staticmethod
    def to_dict(BetfairTicker obj):
        return BetfairTicker.to_dict_c(obj)

    @staticmethod
    cdef BetfairTicker from_dict_c(dict values):
        Condition.not_none(values, "values")
        cdef bytes info = values["info"]
        return BetfairTicker(
            instrument_id=InstrumentId.from_str_c(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            last_traded_price=Price.from_str_c(values["last_traded_price"]),
            traded_volume=Quantity.from_str_c(values["traded_volume"]),
            info=orjson.loads(info) if info is not None else None,
        )

    @staticmethod
    cdef dict to_dict_c(BetfairTicker obj):
        Condition.not_none(obj, "obj")
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "last_traded_price": str(obj.last_traded_price),
            "traded_volume": str(obj.traded_volume),
            "info": orjson.dumps(obj.info) if obj.info is not None else None,
        }

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


register_serializable_object(BSPOrderBookDelta, BSPOrderBookDelta.to_dict, BSPOrderBookDelta.from_dict)
register_serializable_object(BetfairTicker, BetfairTicker.to_dict, BetfairTicker.from_dict)
register_parquet(cls=BetfairTicker, schema=BetfairTicker.schema())
