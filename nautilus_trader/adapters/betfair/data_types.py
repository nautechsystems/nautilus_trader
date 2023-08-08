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
from enum import Enum
from typing import Optional

import pyarrow as pa

# fmt: off
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.book import BookOrder
from nautilus_trader.model.data.book import OrderBookDeltas
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import book_action_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.serialization.arrow.serializer import make_dict_serializer
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.base import register_serializable_object


# fmt: on


class SubscriptionStatus(Enum):
    """
    Represents a `Betfair` subscription status.
    """

    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


class BSPOrderBookDeltas(OrderBookDeltas):
    """
    Represents a `Betfair` BSP order book delta.
    """

    @staticmethod
    def from_dict(values) -> "BSPOrderBookDeltas":
        PyCondition.not_none(values, "values")
        action: BookAction = book_action_from_str(values["action"])
        if action != BookAction.CLEAR:
            book_dict = {
                "price": str(values["price"]),
                "size": str(values["size"]),
                "side": values["side"],
                "order_id": values["order_id"],
            }
            book_order = BookOrder.from_dict(book_dict)
        else:
            book_order = None
        return BSPOrderBookDeltas(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            action=action,
            order=book_order,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj) -> dict:
        values = OrderBookDeltas.to_dict(obj)
        values["type"] = obj.__class__.__name__
        return values

    @classmethod
    def schema(cls) -> pa.Schema:
        return pa.schema(
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
            metadata={"type": "OrderBookDelta"},
        )


class BetfairTicker(Ticker):
    """
    Represents a `Betfair` ticker.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        last_traded_price: Optional[float] = None,
        traded_volume: Optional[float] = None,
        starting_price_near: Optional[float] = None,
        starting_price_far: Optional[float] = None,
    ):
        super().__init__(instrument_id=instrument_id, ts_event=ts_event, ts_init=ts_init)
        self.last_traded_price = last_traded_price
        self.traded_volume = traded_volume
        self.starting_price_near = starting_price_near
        self.starting_price_far = starting_price_far

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.dictionary(pa.int8(), pa.string()),
                "ts_event": pa.uint64(),
                "ts_init": pa.uint64(),
                "last_traded_price": pa.float64(),
                "traded_volume": pa.float64(),
                "starting_price_near": pa.float64(),
                "starting_price_far": pa.float64(),
            },
            metadata={"type": "BetfairTicker"},
        )

    @classmethod
    def from_dict(cls, values: dict):
        return cls(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            last_traded_price=values["last_traded_price"] if values["last_traded_price"] else None,
            traded_volume=values["traded_volume"] if values["traded_volume"] else None,
            starting_price_near=values["starting_price_near"]
            if values["starting_price_near"]
            else None,
            starting_price_far=values["starting_price_far"]
            if values["starting_price_far"]
            else None,
        )

    @staticmethod
    def to_dict(self: "BetfairTicker"):
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
            "last_traded_price": self.last_traded_price,
            "traded_volume": self.traded_volume,
            "starting_price_near": self.starting_price_near,
            "starting_price_far": self.starting_price_far,
        }


class BetfairStartingPrice(Data):
    """
    Represents the realised Betfair Starting Price.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        bsp: Optional[float] = None,
    ):
        super().__init__()
        self._ts_event = ts_event
        self._ts_init = ts_init
        self.instrument_id: InstrumentId = instrument_id
        self.bsp = bsp

    @property
    def ts_init(self) -> int:
        return self._ts_init

    @property
    def ts_event(self) -> int:
        return self._ts_event

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.dictionary(pa.int8(), pa.string()),
                "ts_event": pa.uint64(),
                "ts_init": pa.uint64(),
                "bsp": pa.float64(),
            },
            metadata={"type": "BetfairStartingPrice"},
        )

    @classmethod
    def from_dict(cls, values: dict):
        return cls(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            bsp=values["bsp"] if values["bsp"] else None,
        )

    def to_dict(self):
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
            "bsp": self.bsp,
        }


# Register serialization/parquet BetfairTicker
register_arrow(
    cls=BetfairTicker,
    schema=BetfairTicker.schema(),
    serializer=make_dict_serializer(schema=BetfairTicker.schema()),
)

# Register serialization/parquet BetfairStartingPrice
register_arrow(
    cls=BetfairStartingPrice,
    schema=BetfairStartingPrice.schema(),
    serializer=make_dict_serializer(schema=BetfairStartingPrice.schema()),
)

# Register serialization/parquet BSPOrderBookDeltas
register_serializable_object(
    BSPOrderBookDeltas,
    BSPOrderBookDeltas.to_dict,
    BSPOrderBookDeltas.from_dict,
)

register_arrow(
    cls=BSPOrderBookDeltas,
    serializer=BSPOrderBookDeltas.to_dict,
    deserializer=BSPOrderBookDeltas.from_dict,
    schema=BSPOrderBookDeltas.schema(),
)
