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

from io import BytesIO

import pyarrow as pa

from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.data import OrderBookDelta


def test_legacy_deltas_to_record_batch_reader() -> None:
    # Arrange
    ticks = [
        OrderBookDelta.from_dict(
            {
                "type": "OrderBookDelta",
                "instrument_id": "1.166564490-237491-0.0.BETFAIR",
                "action": "CLEAR",
                "order": {
                    "side": "NO_ORDER_SIDE",
                    "price": "0",
                    "size": "0",
                    "order_id": 0,
                },
                "flags": 32,
                "sequence": 0,
                "ts_event": 1576840503572000000,
                "ts_init": 1576840503572000000,
            },
        ),
    ]

    # Act
    batch_bytes = nautilus_pyo3.pyobjects_to_arrow_record_batch_bytes(ticks)
    reader = pa.ipc.open_stream(BytesIO(batch_bytes))

    # Assert
    assert len(ticks) == 1
    assert len(reader.read_all()) == len(ticks)
    reader.close()
