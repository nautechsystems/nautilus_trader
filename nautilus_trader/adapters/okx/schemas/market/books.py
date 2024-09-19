from decimal import Decimal

import msgspec

from nautilus_trader.core.datetime import millis_to_nanos
from nautilus_trader.model.data import BookOrder
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import BookAction
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class OKXOrderBookSnapshot(msgspec.Struct, frozen=True):
    asks: list[list[str]]
    bids: list[list[str]]
    ts: str

    def parse_to_snapshot(
        self,
        instrument_id: InstrumentId,
        ts_init: int,
    ) -> OrderBookDeltas:
        bids = [
            BookOrder(OrderSide.BUY, Price.from_str(o[0]), Quantity.from_str(o[1]), 0)
            for o in self.bids or []
        ]
        asks = [
            BookOrder(OrderSide.SELL, Price.from_str(o[0]), Quantity.from_str(o[1]), 0)
            for o in self.asks or []
        ]

        deltas = [OrderBookDelta.clear(instrument_id, ts_init, ts_init, 0)]

        deltas += [
            OrderBookDelta(
                instrument_id,
                BookAction.ADD,
                o,
                flags=0,
                sequence=0,
                ts_event=millis_to_nanos(Decimal(self.ts)),
                ts_init=ts_init,
            )
            for o in bids + asks
        ]
        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class OKXOrderBookSnapshotResponse(msgspec.Struct):
    code: str
    msg: str
    data: list[OKXOrderBookSnapshot]
