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
from pathlib import Path
from typing import Any

import pytest

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import GreeksConvention
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import NautilusDataType
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import register_custom_data_class
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import RustTestCustomData
from nautilus_trader.persistence import StreamingFeatherWriter


def _writer(path: Path, **kwargs: Any) -> StreamingFeatherWriter:
    path.mkdir(parents=True)
    return StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        **kwargs,
    )


@customdataclass()
class StreamSignal:
    """
    Python-defined custom data for streaming writer tests.
    """

    value: float = 0.0


def test_streaming_feather_writer_option_greeks_marks_run_non_empty(tmp_path: Path) -> None:
    """
    Verify streaming feather writer option greeks marks run non empty.
    """
    path = tmp_path / "backtest" / "run-greeks"
    writer = _writer(path)
    greeks = OptionGreeks(
        InstrumentId.from_str("BTC-20260529-100000-C.OKX"),
        0.55,
        0.012,
        3.4,
        -1.2,
        0.01,
        0.64,
        None,
        0.66,
        100_000.0,
        None,
        90,
        91,
        GreeksConvention.PRICE_ADJUSTED,
    )

    writer.write(greeks)
    writer.flush()

    manifest_path = path / "_nautilus_run_manifest.json"
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    assert {
        key: manifest[key] for key in ["schema_version", "kind", "instance_id", "status", "empty"]
    } == {
        "schema_version": 1,
        "kind": "backtest",
        "instance_id": "run-greeks",
        "status": "in_progress",
        "empty": False,
    }
    writer.close()
    manifest = json.loads(manifest_path.read_text(encoding="utf-8"))
    assert {
        key: manifest[key] for key in ["schema_version", "kind", "instance_id", "status", "empty"]
    } == {
        "schema_version": 1,
        "kind": "backtest",
        "instance_id": "run-greeks",
        "status": "completed",
        "empty": False,
    }
    assert ParquetDataCatalog(str(tmp_path)).list_backtest_runs() == ["run-greeks"]


def test_streaming_feather_writer_accepts_rust_custom_data(tmp_path: Path) -> None:
    """
    Verify streaming feather writer accepts rust custom data.
    """
    register_custom_data_class(RustTestCustomData)
    staging = tmp_path / "backtest" / "run-rust-custom"
    writer = _writer(staging)
    instrument_id = InstrumentId.from_str("RUST.STREAM")
    data_type = DataType("RustTestCustomData", None, str(instrument_id))
    writer.write(CustomData(data_type, RustTestCustomData(instrument_id, 1.25, True, 1, 1)))
    writer.write(CustomData(data_type, RustTestCustomData(instrument_id, 2.5, False, 2, 2)))
    writer.close()

    assert list(staging.rglob("*.feather"))

    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.convert_stream_to_data("run-rust-custom", NautilusDataType.Custom("RustTestCustomData"))

    result = catalog.query_custom_data("RustTestCustomData")
    assert [item.data.value for item in result] == [1.25, 2.5]
    assert all(isinstance(item.data, RustTestCustomData) for item in result)


def test_streaming_feather_writer_accepts_python_custom_data(tmp_path: Path) -> None:
    """
    Verify streaming feather writer accepts python custom data.
    """
    register_custom_data_class(StreamSignal)
    staging = tmp_path / "backtest" / "run-py-custom"
    writer = _writer(staging)
    data_type = DataType("StreamSignal")
    writer.write(CustomData(data_type, StreamSignal(ts_event=11, ts_init=12, value=42.5)))
    writer.close()

    assert list(staging.rglob("*.feather"))

    catalog = ParquetDataCatalog(str(tmp_path))
    catalog.convert_stream_to_data("run-py-custom", NautilusDataType.Custom("StreamSignal"))

    result = catalog.query_custom_data("StreamSignal")
    assert len(result) == 1
    assert result[0].data.value == 42.5
    assert result[0].data.ts_event == 11
    assert result[0].data.ts_init == 12


@pytest.mark.parametrize("include", ["custom", "RustTestCustomData", "custom/RustTestCustomData"])
def test_streaming_feather_writer_include_types_match_custom_data(
    tmp_path: Path,
    include: str,
) -> None:
    """
    Verify streaming feather writer include types match custom data.
    """
    register_custom_data_class(RustTestCustomData)
    staging = tmp_path / "backtest" / "run-filtered"
    writer = _writer(staging, include_types=[include])
    instrument_id = InstrumentId.from_str("RUST.FILTER")
    data_type = DataType("RustTestCustomData", None, str(instrument_id))
    writer.write(CustomData(data_type, RustTestCustomData(instrument_id, 1.25, True, 1, 1)))
    writer.close()

    assert list(staging.rglob("*.feather"))


def test_streaming_feather_writer_rejects_unregistered_custom_data(tmp_path: Path) -> None:
    """
    Verify streaming feather writer rejects unregistered custom data.
    """

    class StreamNeverRegistered:
        """
        Custom payload that is never registered for Arrow encoding.
        """

        def __init__(self) -> None:
            self.ts_event = 1
            self.ts_init = 1

    staging = tmp_path / "backtest" / "run-unregistered"
    writer = _writer(staging)
    data_type = DataType("StreamNeverRegistered")

    with pytest.raises(OSError, match="Failed to write CustomData"):
        writer.write(CustomData(data_type, StreamNeverRegistered()))

    writer.close()

    assert list(staging.rglob("*.feather")) == []


def test_streaming_feather_writer_include_types_drop_custom_data(tmp_path: Path) -> None:
    """
    Verify streaming feather writer include types drop custom data.
    """
    register_custom_data_class(RustTestCustomData)
    staging = tmp_path / "backtest" / "run-dropped"
    writer = _writer(staging, include_types=["quotes"])
    instrument_id = InstrumentId.from_str("RUST.DROP")
    data_type = DataType("RustTestCustomData", None, str(instrument_id))
    writer.write(CustomData(data_type, RustTestCustomData(instrument_id, 1.25, True, 1, 1)))
    writer.close()

    assert list(staging.rglob("*.feather")) == []
