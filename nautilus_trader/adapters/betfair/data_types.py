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

import pyarrow as pa

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import book_action_from_str
from nautilus_trader.model.enums import book_type_from_str
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.orderbook.data import BookOrder
from nautilus_trader.model.orderbook.data import OrderBookDelta
from nautilus_trader.model.orderbook.data import OrderBookDeltas
from nautilus_trader.serialization.arrow.serializer import register_parquet
from nautilus_trader.serialization.base import register_serializable_object


class SubscriptionStatus(Enum):
    """
    Represents a `Betfair` subscription status.
    """

    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2


class InstrumentSearch(Data):
    """
    Represents a `Betfair` instrument search.
    """

    def __init__(
        self,
        instruments,
        ts_event,
        ts_init,
    ):
        super().__init__(ts_event, ts_init)
        self.instruments = instruments


class BSPOrderBookDeltas(OrderBookDeltas):
    """
    Represents a batch of Betfair BSP order book delta.
    """

    pass


class BSPOrderBookDelta(OrderBookDelta):
    """
    Represents a `Betfair` BSP order book delta.
    """

    @staticmethod
    def from_dict(values) -> "BSPOrderBookDelta":
        PyCondition.not_none(values, "values")
        action: BookAction = book_action_from_str(values["action"])
        order: BookOrder = (
            BookOrder.from_dict(
                {
                    "price": values["order_price"],
                    "size": values["order_size"],
                    "side": values["order_side"],
                    "id": values["order_id"],
                },
            )
            if values["action"] != "CLEAR"
            else None
        )
        return BSPOrderBookDelta(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            book_type=book_type_from_str(values["book_type"]),
            action=action,
            order=order,
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
        )

    @staticmethod
    def to_dict(obj) -> dict:
        values = OrderBookDelta.to_dict(obj)
        values["type"] = obj.__class__.__name__
        return values


class BetfairTicker(Ticker):
    """
    Represents a `Betfair` ticker.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        last_traded_price: Price = None,
        traded_volume: Quantity = None,
    ):
        super().__init__(instrument_id=instrument_id, ts_event=ts_event, ts_init=ts_init)
        self.last_traded_price = last_traded_price
        self.traded_volume = traded_volume

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.dictionary(pa.int8(), pa.string()),
                "ts_event": pa.uint64(),
                "ts_init": pa.uint64(),
                "last_traded_price": pa.string(),
                "traded_volume": pa.string(),
            },
            metadata={"type": "BetfairTicker"},
        )

    @classmethod
    def from_dict(cls, values: dict):
        return cls(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            last_traded_price=Price.from_str(values["last_traded_price"])
            if values["last_traded_price"]
            else None,
            traded_volume=Quantity.from_str(values["traded_volume"])
            if values["traded_volume"]
            else None,
        )

    def to_dict(self):
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
            "last_traded_price": str(self.last_traded_price) if self.last_traded_price else None,
            "traded_volume": str(self.traded_volume) if self.traded_volume else None,
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
        bsp: float = None,
    ):
        super().__init__(ts_event=ts_event, ts_init=ts_init)
        self.instrument_id: InstrumentId = instrument_id
        self.bsp = bsp

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.dictionary(pa.int8(), pa.string()),
                "ts_event": pa.uint64(),
                "ts_init": pa.uint64(),
                "bsp": pa.float(),
            },
            metadata={"type": "BetfairStartingPrice"},
        )

    @classmethod
    def from_dict(cls, values: dict):
        return cls(
            instrument_id=InstrumentId.from_str(values["instrument_id"]),
            ts_event=values["ts_event"],
            ts_init=values["ts_init"],
            bsp=Price.from_str(values["bsp"]) if values["bsp"] else None,
        )

    def to_dict(self):
        return {
            "type": type(self).__name__,
            "instrument_id": self.instrument_id.value,
            "ts_event": self.ts_event,
            "ts_init": self.ts_init,
            "bsp": str(self.bsp) if self.bsp else None,
        }


BSP_SCHEMA = pa.schema(
    {
        "instrument_id": pa.string(),
        "ts_event": pa.uint64(),
        "ts_init": pa.uint64(),
        "action": pa.string(),
        "order_side": pa.string(),
        "order_price": pa.float64(),
        "order_size": pa.float64(),
        "order_id": pa.string(),
        "book_type": pa.string(),
    },
    metadata={"type": "BSPOrderBookDelta"},
)


register_serializable_object(BetfairTicker, BetfairTicker.to_dict, BetfairTicker.from_dict)
register_parquet(cls=BetfairTicker, schema=BetfairTicker.schema())

register_serializable_object(
    BetfairStartingPrice,
    BetfairStartingPrice.to_dict,
    BetfairStartingPrice.from_dict,
)
register_parquet(cls=BetfairStartingPrice, schema=BetfairStartingPrice.schema())


register_serializable_object(
    BSPOrderBookDeltas,
    BSPOrderBookDeltas.to_dict,
    BSPOrderBookDeltas.from_dict,
)
register_parquet(cls=BSPOrderBookDeltas, schema=BSP_SCHEMA)
