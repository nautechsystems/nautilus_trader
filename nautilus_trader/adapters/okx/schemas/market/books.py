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
