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

from pathlib import Path
from typing import Optional

import pyarrow as pa
import pytest

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.persistence import ParquetDataCatalog


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


@customdataclass
class InheritedData:
    """
    Base fields included in a derived custom-data schema.
    """

    count: int = 7


@customdataclass
class DerivedData(InheritedData):
    """
    Custom data extending a base dataclass.
    """

    value: int = 11


def test_inherited_custom_data_fields_roundtrip() -> None:
    """
    Preserve non-default inherited values through Arrow.
    """
    original = DerivedData(ts_event=17, ts_init=29, count=43, value=61)
    batch = original.encode_record_batch_py([original])
    [restored] = DerivedData.decode_record_batch_py({}, batch)

    assert batch.schema.names == ["count", "value", "type", "ts_event", "ts_init"]
    assert restored == original
    assert restored.ts_event == 17
    assert restored.ts_init == 29


@customdataclass
class OptionalAnnotatedData:
    """
    Custom data with optional union field annotations.
    """

    count: int | None = None
    ratio: float | None = None
    enabled: bool | None = None
    label: str | None = None
    instrument_id: InstrumentId | None = None
    values: dict[str, int] | None = None


OptionalInstrumentId = Optional[InstrumentId]  # noqa: UP045


@customdataclass
class TypingOptionalAnnotatedData:
    """
    Custom data using typing.Optional and reversed union order.
    """

    instrument_id: OptionalInstrumentId = None
    note: None | str = None  # noqa: RUF036


@customdataclass
class OptionalCatalogSignal:
    """
    Custom data with optional fields for catalog persistence tests.
    """

    instrument_id: InstrumentId | None = None
    score: float | None = None
    metadata: dict[str, int] | None = None


@pytest.mark.parametrize("representation", ["json", "arrow"])
@pytest.mark.parametrize("populated", [True, False])
def test_optional_custom_data_annotations_roundtrip(
    representation: str,
    populated: bool,
) -> None:
    """
    Preserve optional field values and None across JSON and Arrow round trips.
    """
    if populated:
        original = OptionalAnnotatedData(
            ts_event=17,
            ts_init=29,
            count=43,
            ratio=1.25,
            enabled=True,
            label="signal",
            instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
            values={"bid": 61, "ask": 73},
        )
    else:
        original = OptionalAnnotatedData(ts_event=31, ts_init=47)

    schema = OptionalAnnotatedData.arrow_schema_py()

    if representation == "json":
        restored = OptionalAnnotatedData.from_json(original.to_json())
    else:
        batch = original.encode_record_batch_py([original])
        [restored] = OptionalAnnotatedData.decode_record_batch_py({}, batch)

    assert schema == pa.schema(
        [
            pa.field("count", pa.int64()),
            pa.field("ratio", pa.float64()),
            pa.field("enabled", pa.bool_()),
            pa.field("label", pa.string()),
            pa.field("instrument_id", pa.string()),
            pa.field("values", pa.string()),
            pa.field("type", pa.string(), nullable=False),
            pa.field("ts_event", pa.timestamp("ns", tz="UTC"), nullable=False),
            pa.field("ts_init", pa.timestamp("ns", tz="UTC"), nullable=False),
        ],
    )
    assert restored == original
    assert restored.ts_event == original.ts_event
    assert restored.ts_init == original.ts_init
    if populated:
        assert type(restored.instrument_id) is InstrumentId
        assert type(restored.values) is dict


@pytest.mark.parametrize("representation", ["json", "arrow"])
def test_typing_optional_and_reversed_union_roundtrip(representation: str) -> None:
    """
    Support typing.Optional and None | T union field annotations.
    """
    original = TypingOptionalAnnotatedData(
        ts_event=11,
        ts_init=13,
        instrument_id=InstrumentId.from_str("ETHUSDT.BINANCE"),
        note="ok",
    )

    if representation == "json":
        restored = TypingOptionalAnnotatedData.from_json(original.to_json())
    else:
        batch = original.encode_record_batch_py([original])
        [restored] = TypingOptionalAnnotatedData.decode_record_batch_py({}, batch)

    assert restored == original
    assert type(restored.instrument_id) is InstrumentId


def test_optional_custom_data_catalog_and_json_envelope_roundtrip(tmp_path: Path) -> None:
    """
    Round-trip optional fields through the Parquet catalog and CustomData JSON.
    """
    register_custom_data_class(OptionalCatalogSignal)
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("OptionalCatalogSignal")

    empty = OptionalCatalogSignal(ts_event=100, ts_init=101)

    populated = OptionalCatalogSignal(
        ts_event=200,
        ts_init=201,
        instrument_id=InstrumentId.from_str("EUR/USD.SIM"),
        score=2.5,
        metadata={"window": 5},
    )

    wrapped = [CustomData(data_type, empty), CustomData(data_type, populated)]

    catalog.write_custom_data(wrapped)
    queried = catalog.query_custom_data(NautilusDataType.Custom("OptionalCatalogSignal"))

    assert [item.data for item in queried] == [empty, populated]
    assert type(queried[1].data.instrument_id) is InstrumentId
    assert type(queried[1].data.metadata) is dict

    json_restored = [CustomData.from_json_bytes(item.to_json_bytes()).data for item in wrapped]
    assert json_restored == [empty, populated]
    assert type(json_restored[1].instrument_id) is InstrumentId
    assert type(json_restored[1].metadata) is dict
