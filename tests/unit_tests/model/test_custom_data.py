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


import json
from dataclasses import field

import pyarrow as pa

from nautilus_trader.core.data import Data
from nautilus_trader.model.custom import customdataclass
from nautilus_trader.model.identifiers import InstrumentId


@customdataclass
class GreeksTestData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    delta: float = 0.0


@customdataclass
class JsonTestData(Data):
    name: str = ""
    payload: dict[str, object] = field(default_factory=dict)


def test_customdata_decorator_properties() -> None:
    # Arrange, Act
    data = GreeksTestData(ts_event=1, ts_init=2)

    # Assert
    assert data.ts_event == 1
    assert data.ts_init == 2


def test_customdata_decorator_dict() -> None:
    # Arrange
    data = GreeksTestData(1, 2, InstrumentId.from_str("ES.GLBX"), 0.0)

    # Act
    data_dict = data.to_dict()

    # Assert
    assert data_dict == {
        "instrument_id": "ES.GLBX",
        "delta": 0.0,
        "type": "GreeksTestData",
        "ts_event": 1,
        "ts_init": 2,
    }


def test_customdata_repr() -> None:
    # Arrange
    data = GreeksTestData(ts_event=1715248800000000000, ts_init=1715248860000000000)

    # Act
    repr = str(data)

    # Assert
    assert (
        repr
        == "GreeksTestData(instrument_id=InstrumentId('ES.GLBX'), delta=0.0, ts_event=2024-05-09T10:00:00.000000000Z, ts_init=2024-05-09T10:01:00.000000000Z)"
    )


def test_customdata_decorator_dict_identity() -> None:
    # Arrange
    data = GreeksTestData(
        ts_event=1,
        ts_init=2,
        instrument_id=InstrumentId.from_str("CL.GLBX"),
        delta=1000.0,
    )

    # Act
    new_data = GreeksTestData.from_dict(data.to_dict())

    # Assert
    assert new_data == data


def test_customdata_decorator_bytes_identity() -> None:
    # Arrange
    data = GreeksTestData(
        ts_event=1,
        ts_init=2,
        instrument_id=InstrumentId.from_str("CL.GLBX"),
        delta=1000.0,
    )

    # Act
    new_data = GreeksTestData.from_bytes(data.to_bytes())

    # Assert
    assert new_data == data


def test_customdata_decorator_arrow_identity() -> None:
    # Arrange
    data = GreeksTestData(
        ts_event=1,
        ts_init=2,
        instrument_id=InstrumentId.from_str("CL.GLBX"),
        delta=1000.0,
    )

    # Act
    new_data = GreeksTestData.from_arrow(data.to_arrow())[0]

    # Assert
    assert new_data == data


def test_customdata_decorator_arrow_identity_with_dict_payload() -> None:
    # Arrange
    payload = {
        "symbol": "CLM4",
        "nested": {"count": 2, "enabled": True},
        "values": [1, 2, 3],
    }
    data = JsonTestData(
        ts_event=1,
        ts_init=2,
        name="snapshot",
        payload=payload,
    )

    # Act
    data_dict = data.to_dict()
    arrow_batch = data.to_arrow()
    arrow_row = arrow_batch.to_pylist()[0]
    new_data = JsonTestData.from_arrow(arrow_batch)[0]

    # Assert
    assert data_dict == {
        "name": "snapshot",
        "payload": payload,
        "type": "JsonTestData",
        "ts_event": 1,
        "ts_init": 2,
    }
    assert JsonTestData._schema.field("payload").type == pa.string()
    assert arrow_row["payload"] == json.dumps(payload, sort_keys=True)
    assert new_data == data
    assert isinstance(new_data.payload, dict)
