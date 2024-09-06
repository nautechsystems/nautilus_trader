# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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

import msgspec

from nautilus_trader.adapters.bybit.common.parsing import parse_bybit_delta
from nautilus_trader.model.data import OrderBookDelta
from nautilus_trader.model.data import OrderBookDeltas
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import RecordFlag
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


class BybitDeltasList(msgspec.Struct, array_like=True):
    # Symbol
    s: str
    # Bids
    b: list[list[str]]
    # Asks
    a: list[list[str]]
    # Update ID (1 = service restart - clear book)
    u: int
    # Cross sequence
    seq: int

    def parse_to_deltas(
        self,
        instrument_id: InstrumentId,
        ts_event: int,
        ts_init: int,
        snapshot: bool = False,
    ) -> OrderBookDeltas:
        bids_raw = [(Price.from_str(d[0]), Quantity.from_str(d[1])) for d in self.b]
        asks_raw = [(Price.from_str(d[0]), Quantity.from_str(d[1])) for d in self.a]

        num_bids_raw = len(bids_raw)
        num_asks_raw = len(asks_raw)
        deltas: list[OrderBookDelta] = []

        if snapshot:
            deltas.append(OrderBookDelta.clear(instrument_id, 0, ts_event, ts_init))

        for idx, bid in enumerate(bids_raw):
            flags = 0
            if idx == num_bids_raw - 1 and num_asks_raw == 0:
                # F_LAST, 1 << 7
                # Last message in the packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_bybit_delta(
                instrument_id=instrument_id,
                values=bid,
                side=OrderSide.BUY,
                update_id=self.u,
                flags=flags,
                sequence=self.seq,
                ts_event=ts_event,
                ts_init=ts_init,
                snapshot=snapshot,
            )
            deltas.append(delta)

        for idx, ask in enumerate(asks_raw):
            flags = 0
            if idx == num_asks_raw - 1:
                # F_LAST, 1 << 7
                # Last message in the book event or packet from the venue for a given `instrument_id`
                flags = RecordFlag.F_LAST

            delta = parse_bybit_delta(
                instrument_id=instrument_id,
                values=ask,
                side=OrderSide.SELL,
                update_id=self.u,
                flags=flags,
                sequence=self.seq,
                ts_event=ts_event,
                ts_init=ts_init,
                snapshot=snapshot,
            )
            deltas.append(delta)

        return OrderBookDeltas(instrument_id=instrument_id, deltas=deltas)


class BybitOrderBookResponse(msgspec.Struct):
    # Topic name
    topic: str
    # Data type
    type: str
    # The timestamp (UNIX milliseconds) that the system generated the data
    ts: int
    data: BybitDeltasList
    # The timestamp from the match engine when this orderbook data is produced
    cts: int
