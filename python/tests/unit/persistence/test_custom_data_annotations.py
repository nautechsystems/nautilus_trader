#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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
"""
Regression tests for postponed custom-data annotations.
"""

from __future__ import annotations

import pyarrow as pa
import pytest

from nautilus_trader.model import InstrumentId
from nautilus_trader.model.custom import customdataclass


@customdataclass
class AnnotatedData:
    """
    Custom data with postponed field annotations.
    """

    count: int
    instrument_id: InstrumentId
    values: dict[str, int]


@pytest.mark.parametrize("representation", ["json", "arrow"])
def test_postponed_custom_data_annotations_roundtrip(representation: str) -> None:
    """
    Preserve declared fields through JSON and Arrow round trips.
    """
    original = AnnotatedData(
        ts_event=17,
        ts_init=29,
        count=43,
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
        values={"bid": 61, "ask": 73},
    )
    schema = AnnotatedData.arrow_schema_py()

    if representation == "json":
        restored = AnnotatedData.from_json(original.to_json())
    else:
        batch = original.encode_record_batch_py([original])
        [restored] = AnnotatedData.decode_record_batch_py({}, batch)

    assert schema == pa.schema(
        [
            pa.field("count", pa.int64()),
            pa.field("instrument_id", pa.string()),
            pa.field("values", pa.string()),
            pa.field("type", pa.string(), nullable=False),
            pa.field("ts_event", pa.timestamp("ns", tz="UTC"), nullable=False),
            pa.field("ts_init", pa.timestamp("ns", tz="UTC"), nullable=False),
        ],
    )
    assert restored == original
    assert restored.ts_event == 17
    assert restored.ts_init == 29
    assert type(restored.instrument_id) is InstrumentId
    assert type(restored.values) is dict
