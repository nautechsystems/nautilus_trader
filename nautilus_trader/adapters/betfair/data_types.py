# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from __future__ import annotations

from enum import Enum

import pyarrow as pa

from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.serialization.arrow.serializer import make_dict_deserializer
from nautilus_trader.serialization.arrow.serializer import make_dict_serializer
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.base import register_serializable_type


class SubscriptionStatus(Enum):
    """
    Represents a Betfair subscription status.
    """

    UNSUBSCRIBED = 0
    PENDING_STARTUP = 1
    RUNNING = 2
    SUBSCRIBED = 3


class BSPOrderBookDelta(OrderBookDelta):
    @staticmethod
    def from_batch(batch: pa.RecordBatch) -> list[BSPOrderBookDelta]:
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
                flags=0,
                sequence=0,
                ts_event=batch["ts_event"].to_pylist()[idx],
                ts_init=batch["ts_init"].to_pylist()[idx],
            )
            data.append(delta)
        return data

    @staticmethod
    def to_batch(obj: BSPOrderBookDelta) -> pa.RecordBatch:
        metadata = {
            b"instrument_id": obj.instrument_id.value.encode(),
            b"price_precision": str(obj.order.price.precision).encode(),
            b"size_precision": str(obj.order.size.precision).encode(),
        }
        schema = BSPOrderBookDelta.schema().with_metadata(metadata)
        return pa.RecordBatch.from_pylist(
            [
                {
                    "action": obj.action,
                    "side": obj.order.side,
                    "price": obj.order.price.raw,
                    "size": obj.order.size.raw,
                    "order_id": obj.order.order_id,
                    "flags": obj.flags,
                    "ts_event": obj.ts_event,
                    "ts_init": obj.ts_init,
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


class BetfairTicker(Data):
    """
    Represents a Betfair ticker.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        last_traded_price: float | None = None,
        traded_volume: float | None = None,
        starting_price_near: float | None = None,
        starting_price_far: float | None = None,
    ):
        self.instrument_id = instrument_id
        self.last_traded_price = last_traded_price
        self.traded_volume = traded_volume
        self.starting_price_near = starting_price_near
        self.starting_price_far = starting_price_far
        self._ts_event = ts_event
        self._ts_init = ts_init

    def __eq__(self, other: object) -> bool:
        if not isinstance(other, BetfairTicker):
            return False
        return self.instrument_id == other.instrument_id

    def __hash__(self) -> int:
        return hash(self.instrument_id)

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

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
            starting_price_near=(
                values["starting_price_near"] if values["starting_price_near"] else None
            ),
            starting_price_far=(
                values["starting_price_far"] if values["starting_price_far"] else None
            ),
        )

    @staticmethod
    def to_dict(obj: BetfairTicker):
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "ts_event": obj._ts_event,
            "ts_init": obj._ts_init,
            "last_traded_price": obj.last_traded_price,
            "traded_volume": obj.traded_volume,
            "starting_price_near": obj.starting_price_near,
            "starting_price_far": obj.starting_price_far,
        }

    def __repr__(self):
        return (
            f"BetfairTicker(instrument_id={self.instrument_id.value}, ltp={self.last_traded_price}, "
            f"tv={self.traded_volume}, spn={self.starting_price_near}, spf={self.starting_price_far},"
            f" ts_init={self.ts_init})"
        )


class BetfairStartingPrice(Data):
    """
    Represents the realized Betfair Starting Price.
    """

    def __init__(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        bsp: float | None = None,
    ):
        self.instrument_id: InstrumentId = instrument_id
        self.bsp = bsp
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

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
    def to_dict(obj):
        return {
            "type": type(obj).__name__,
            "instrument_id": obj.instrument_id.value,
            "ts_event": obj.ts_event,
            "ts_init": obj.ts_init,
            "bsp": obj.bsp,
        }


# Register serialization/parquet BetfairTicker
register_serializable_type(
    BetfairTicker,
    BetfairTicker.to_dict,
    BetfairTicker.from_dict,
)

register_arrow(
    data_cls=BetfairTicker,
    schema=BetfairTicker.schema(),
    encoder=make_dict_serializer(schema=BetfairTicker.schema()),
    decoder=make_dict_deserializer(BetfairTicker),
)

# Register serialization/parquet BetfairStartingPrice
register_serializable_type(
    BetfairStartingPrice,
    BetfairStartingPrice.to_dict,
    BetfairStartingPrice.from_dict,
)

register_arrow(
    data_cls=BetfairStartingPrice,
    schema=BetfairStartingPrice.schema(),
    encoder=make_dict_serializer(schema=BetfairStartingPrice.schema()),
    decoder=make_dict_deserializer(BetfairStartingPrice),
)


# Register serialization/parquet BSPOrderBookDeltas
register_serializable_type(
    BSPOrderBookDelta,
    BSPOrderBookDelta.to_dict,
    BSPOrderBookDelta.from_dict,
)

register_arrow(
    data_cls=BSPOrderBookDelta,
    encoder=BSPOrderBookDelta.to_batch,
    decoder=BSPOrderBookDelta.from_batch,
    schema=BSPOrderBookDelta.schema(),
)
