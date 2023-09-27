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
from nautilus_trader.model.data.book import OrderBookDelta
from nautilus_trader.model.data.ticker import Ticker
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.arrow.serializer import make_dict_deserializer
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


class BSPOrderBookDelta(OrderBookDelta):
    @staticmethod
    def from_batch(batch: pa.RecordBatch) -> list["BSPOrderBookDelta"]:
        PyCondition.not_none(batch, "batch")
        data = []
        for idx in range(batch.num_rows):
            instrument_id = InstrumentId.from_str(batch.schema.metadata[b"instrument_id"].decode())
            action: BookAction = BookAction(batch["action"].to_pylist()[idx])
            if action == BookAction.CLEAR:
                book_order = None
            else:
                book_order = BookOrder(
                    price=Price.from_raw(
                        batch["price"].to_pylist()[idx],
                        int(batch.schema.metadata[b"price_precision"]),
                    ),
                    size=Quantity.from_raw(
                        batch["size"].to_pylist()[idx],
                        int(batch.schema.metadata[b"size_precision"]),
                    ),
                    side=batch["side"].to_pylist()[idx],
                    order_id=batch["order_id"].to_pylist()[idx],
                )

            delta = BSPOrderBookDelta(
                instrument_id=instrument_id,
                action=action,
                order=book_order,
                ts_event=batch["ts_event"].to_pylist()[idx],
                ts_init=batch["ts_init"].to_pylist()[idx],
            )
            data.append(delta)
        return data

    @staticmethod
    def to_batch(self: "BSPOrderBookDelta") -> pa.RecordBatch:
        metadata = {
            b"instrument_id": self.instrument_id.value.encode(),
            b"price_precision": str(self.order.price.precision).encode(),
            b"size_precision": str(self.order.size.precision).encode(),
        }
        schema = BSPOrderBookDelta.schema().with_metadata(metadata)
        return pa.RecordBatch.from_pylist(
            [
                {
                    "action": self.action,
                    "side": self.order.side,
                    "price": self.order.price.raw,
                    "size": self.order.size.raw,
                    "order_id": self.order.order_id,
                    "flags": self.flags,
                    "ts_event": self.ts_event,
                    "ts_init": self.ts_init,
                },
            ],
            schema=schema,
        )

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
            metadata={"type": "BSPOrderBookDelta"},
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

    def __repr__(self):
        return (
            f"BetfairTicker(instrument_id={self.instrument_id.value}, ltp={self.last_traded_price}, "
            f"tv={self.traded_volume}, spn={self.starting_price_near}, spf={self.starting_price_far},"
            f" ts_init={self.ts_init})"
        )


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

    @staticmethod
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
    data_cls=BetfairTicker,
    schema=BetfairTicker.schema(),
    serializer=make_dict_serializer(schema=BetfairTicker.schema()),
    deserializer=make_dict_deserializer(BetfairTicker),
)

# Register serialization/parquet BetfairStartingPrice
register_arrow(
    data_cls=BetfairStartingPrice,
    schema=BetfairStartingPrice.schema(),
    serializer=make_dict_serializer(schema=BetfairStartingPrice.schema()),
    deserializer=make_dict_deserializer(BetfairStartingPrice),
)

# Register serialization/parquet BSPOrderBookDeltas
register_serializable_object(
    BSPOrderBookDelta,
    BSPOrderBookDelta.to_dict,
    BSPOrderBookDelta.from_dict,
)

register_arrow(
    data_cls=BSPOrderBookDelta,
    serializer=BSPOrderBookDelta.to_batch,
    deserializer=BSPOrderBookDelta.from_batch,
    schema=BSPOrderBookDelta.schema(),
)
