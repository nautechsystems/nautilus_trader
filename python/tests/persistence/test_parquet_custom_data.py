# -------------------------------------------------------------------------------------------------
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
# -------------------------------------------------------------------------------------------------
"""
Parquet catalog regression tests.
"""

import json
import subprocess
import sys
from pathlib import Path

import pyarrow as pa
import pytest

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData


def test_python_custom_data_query_applies_where_clause(tmp_path: Path) -> None:
    """
    Verify python custom data query applies where clause.
    """
    register_custom_data_class(RustTestCustomData)
    catalog_path = tmp_path / "catalog"
    catalog_path.mkdir()
    catalog = ParquetDataCatalog(str(catalog_path))
    instrument_id = InstrumentId.from_str("RUST.WHERE")
    data_type = DataType("RustTestCustomData", None, str(instrument_id))
    original = [
        RustTestCustomData(instrument_id, 1.23, True, 1, 1),
        RustTestCustomData(instrument_id, 4.56, False, 2, 2),
    ]
    catalog.write_custom_data([CustomData(data_type, item) for item in original])

    result = catalog.query(
        "RustTestCustomData",
        [str(instrument_id)],
        None,
        None,
        "value = 4.56",
        None,
        True,
    )

    assert len(result) == 1
    inner = result[0].data
    assert isinstance(inner, RustTestCustomData)
    assert inner.instrument_id == instrument_id
    assert inner.value == 4.56
    assert inner.flag is False
    assert inner.ts_event == 2
    assert inner.ts_init == 2


class _SensorReadingBase:
    """
    Satisfy the documented `register_custom_data_class` contract and nothing more.
    """

    def __init__(self, sensor_id: str, value: float, ts_event: int, ts_init: int) -> None:
        self.sensor_id = sensor_id
        self.value = value
        self.ts_event = ts_event
        self.ts_init = ts_init

    @classmethod
    def type_name_static(cls) -> str:
        """
        Return the registered type name.
        """
        return cls.__name__

    def to_json(self) -> str:
        """
        Serialize to a JSON string.
        """
        return json.dumps(self.__dict__)

    @classmethod
    def from_json(cls, data: bytes | str | dict) -> "_SensorReadingBase":
        """
        Deserialize from a JSON string or mapping.
        """
        values = json.loads(data) if isinstance(data, (bytes, str)) else data
        return cls(
            values["sensor_id"],
            values["value"],
            values["ts_event"],
            values["ts_init"],
        )

    def encode_record_batch_py(self, items: list) -> pa.RecordBatch:
        """
        Encode the given items to an Arrow record batch.
        """
        timestamp = pa.timestamp("ns", tz="UTC")
        return pa.RecordBatch.from_pydict(
            {
                "sensor_id": [item.sensor_id for item in items],
                "value": [item.value for item in items],
                "ts_event": pa.array([item.ts_event for item in items], timestamp),
                "ts_init": pa.array([item.ts_init for item in items], timestamp),
            },
        )

    @classmethod
    def decode_record_batch_py(cls, _metadata: dict, batch: pa.RecordBatch) -> list:
        """
        Decode the given record batch to a list of instances.
        """
        columns = batch.select(["sensor_id", "value"]).to_pydict()
        return [
            cls(sensor_id, value, ts_event, ts_init)
            for sensor_id, value, ts_event, ts_init in zip(
                columns["sensor_id"],
                columns["value"],
                batch.column(batch.schema.get_field_index("ts_event")).cast("int64").to_pylist(),
                batch.column(batch.schema.get_field_index("ts_init")).cast("int64").to_pylist(),
                strict=True,
            )
        ]


def test_custom_data_write_rejects_class_without_arrow_schema(tmp_path: Path) -> None:
    """
    Verify custom data write rejects class without arrow schema.
    """

    class SensorReadingNoSchema(_SensorReadingBase):
        """
        Collect sensor readings without an Arrow schema.
        """

    register_custom_data_class(SensorReadingNoSchema)
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("SensorReadingNoSchema")
    data = [CustomData(data_type, SensorReadingNoSchema("s1", 42.5, 11, 12))]

    with pytest.raises(OSError, match="registered without an Arrow schema containing ts_init"):
        catalog.write_custom_data(data)

    assert catalog.list_data_types() == []


def test_custom_data_round_trips_with_declared_arrow_schema(tmp_path: Path) -> None:
    """
    Verify custom data round trips with declared arrow schema.
    """

    class SensorReadingWithSchema(_SensorReadingBase):
        """
        Collect sensor readings with a declared Arrow schema.
        """

        @classmethod
        def arrow_schema_py(cls) -> pa.Schema:
            """
            Return the Arrow schema for the encoded batches.
            """
            timestamp = pa.timestamp("ns", tz="UTC")
            return pa.schema(
                [
                    ("sensor_id", pa.string()),
                    ("value", pa.float64()),
                    ("ts_event", timestamp),
                    ("ts_init", timestamp),
                ],
            )

    register_custom_data_class(SensorReadingWithSchema)
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("SensorReadingWithSchema")
    catalog.write_custom_data([CustomData(data_type, SensorReadingWithSchema("s1", 42.5, 11, 12))])

    result = catalog.query_custom_data("SensorReadingWithSchema")

    assert len(result) == 1
    reading = result[0].data
    assert reading.sensor_id == "s1"
    assert reading.value == 42.5
    assert reading.ts_event == 11
    assert reading.ts_init == 12


def test_custom_data_write_rejects_legacy_timestamp_schema(tmp_path: Path) -> None:
    """
    Verify custom data write rejects legacy timestamp schema.
    """

    class SensorReadingLegacyTimestamps(_SensorReadingBase):
        """
        Collect sensor readings with pre-catalog integer timestamps.
        """

        _schema = pa.schema(
            [
                ("sensor_id", pa.string()),
                ("value", pa.float64()),
                ("ts_event", pa.uint64()),
                ("ts_init", pa.uint64()),
            ],
        )

    register_custom_data_class(SensorReadingLegacyTimestamps)
    catalog = ParquetDataCatalog(str(tmp_path))
    data_type = DataType("SensorReadingLegacyTimestamps")
    data = [CustomData(data_type, SensorReadingLegacyTimestamps("s1", 42.5, 11, 12))]

    with pytest.raises(OSError, match="registered with ts_event as UInt64"):
        catalog.write_custom_data(data)

    assert catalog.list_data_types() == []


def test_customdataclass_round_trips_without_pandas(tmp_path: Path) -> None:
    """
    Verify customdataclass round trips without pandas.
    """
    # Sub-microsecond timestamps expose pandas dependence and precision loss
    code = """
import sys


class _PandasBlocker:
    def find_spec(self, name, path=None, target=None):
        if name == "pandas" or name.startswith("pandas."):
            raise ModuleNotFoundError(name)
        return None


sys.meta_path.insert(0, _PandasBlocker())

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.persistence import ParquetDataCatalog


@customdataclass()
class PandasFreeSignal:
    value: float = 0.0


register_custom_data_class(PandasFreeSignal)
catalog = ParquetDataCatalog(sys.argv[1])
catalog.write_custom_data(
    [CustomData(DataType("PandasFreeSignal"), PandasFreeSignal(11, 12, 42.5))],
)

result = catalog.query_custom_data("PandasFreeSignal")
assert len(result) == 1, result
assert result[0].data.value == 42.5
assert result[0].data.ts_event == 11
assert result[0].data.ts_init == 12
assert "pandas" not in sys.modules
"""
    subprocess.run([sys.executable, "-c", code, str(tmp_path)], check=True)
