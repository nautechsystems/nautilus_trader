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
Test data behavior.
"""

import json

import pytest

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import custom_data_backend_kind
from nautilus_trader.model import deserialize_custom_from_json
from nautilus_trader.model import register_custom_data_class


def test_data_type_construction() -> None:
    """
    Test data type construction.
    """
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert dt.type_name == "QuoteTick"
    assert dt.metadata == {"instrument_id": "AUD/USD.SIM"}


def test_data_type_equality() -> None:
    """
    Test data type equality.
    """
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt3 = DataType("QuoteTick", metadata={"instrument_id": "GBP/USD.SIM"})

    assert dt1 == dt2
    assert dt1 != dt3


def test_data_type_hash() -> None:
    """
    Test data type hash.
    """
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert hash(dt1) == hash(dt2)


def test_data_type_topic() -> None:
    """
    Test data type topic.
    """
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert "QuoteTick" in dt.topic
    assert "AUD/USD.SIM" in dt.topic


def test_data_type_identifier() -> None:
    """
    Test data type identifier.
    """
    dt = DataType("QuoteTick", identifier="alpha")

    assert dt.identifier == "alpha"


def test_custom_data_python_backend_and_json_bytes() -> None:
    """
    Test custom data python backend and json bytes.
    """

    class Dummy:
        """
        Collect dummy tests.
        """

        ts_event = 1
        ts_init = 2

        def __repr__(self) -> str:
            """
            Repr.
            """
            return "Dummy()"

    custom = CustomData(DataType("Example"), Dummy())
    payload = custom.to_json_bytes()

    assert custom.data_type.type_name == "Example"
    assert custom.ts_event == 1
    assert custom.ts_init == 2
    assert custom_data_backend_kind(custom) == "python"
    assert b'"type":"Dummy"' in payload


def test_custom_data_python_backend_equality_uses_identity() -> None:
    """
    Test custom data python backend equality uses identity.
    """

    class Dummy:
        """
        Collect dummy tests.
        """

        def __init__(self, value: object) -> None:
            """
            Initialize the instance.
            """
            self.value = value
            self.ts_event = 1
            self.ts_init = 2

    data_type = DataType("Dummy")
    first = Dummy(7)

    assert CustomData(data_type, first) == CustomData(data_type, first)
    assert CustomData(data_type, first) != CustomData(data_type, Dummy(7))


def test_register_custom_data_class_accepts_surface_compatible_class() -> None:
    """
    Test register custom data class accepts surface compatible class.
    """

    class SurfaceCustomData:
        """
        Collect surface custom data tests.
        """

        @classmethod
        def type_name_static(cls) -> str:
            """
            Type name static.
            """
            return "SurfaceCustomData"

        @classmethod
        def from_json(cls, _data: object) -> object:
            """
            From json.
            """
            return cls()

        @classmethod
        def decode_record_batch_py(cls, _metadata: object, _batch: object) -> object:
            """
            Decode record batch py.
            """
            return []

    assert register_custom_data_class(SurfaceCustomData) is None


def test_deserialize_custom_from_json() -> None:
    """
    Test deserialize custom from json.
    """

    class SurfaceCustomDataJson:
        """
        Collect surface custom data json tests.
        """

        def __init__(self, value: object = 0, ts_event: object = 0, ts_init: object = 0) -> None:
            """
            Initialize the instance.
            """
            self.value = value
            self.ts_event = ts_event
            self.ts_init = ts_init

        @classmethod
        def type_name_static(cls) -> str:
            """
            Type name static.
            """
            return "SurfaceCustomDataJson"

        @classmethod
        def from_json(cls, data: object) -> object:
            """
            From json.
            """
            return cls(
                value=data.get("value", 0),
                ts_event=data.get("ts_event", 0),
                ts_init=data.get("ts_init", 0),
            )

        @classmethod
        def decode_record_batch_py(cls, _metadata: object, _batch: object) -> object:
            """
            Decode record batch py.
            """
            return []

    register_custom_data_class(SurfaceCustomDataJson)

    payload = json.dumps(
        {
            "type": "SurfaceCustomDataJson",
            "data_type": {
                "type_name": "SurfaceCustomDataJson",
                "metadata": {"source": "external"},
                "identifier": "feed-a",
            },
            "payload": {"value": 7, "ts_event": 11, "ts_init": 12},
        },
    ).encode()

    custom = deserialize_custom_from_json("SurfaceCustomDataJson", payload)

    assert type(custom).__name__ == "CustomData"
    assert type(custom.data).__name__ == "SurfaceCustomDataJson"
    assert custom.data.value == 7
    assert custom.ts_event == 11
    assert custom.ts_init == 12
    assert custom.data_type.type_name == "SurfaceCustomDataJson"
    assert custom.data_type.metadata == {"source": "external"}
    assert custom.data_type.identifier == "feed-a"


def test_register_custom_data_class_requires_decoder() -> None:
    """
    Test register custom data class requires decoder.
    """

    class MissingDecoder:
        """
        Collect missing decoder tests.
        """

        @classmethod
        def from_json(cls, _data: object) -> object:
            """
            From json.
            """
            return cls()

    with pytest.raises(TypeError, match="decode_record_batch_py"):
        register_custom_data_class(MissingDecoder)


def test_register_custom_data_class_requires_from_json() -> None:
    """
    Test register custom data class requires from json.
    """

    class MissingFromJson:
        """
        Collect missing from json tests.
        """

        @classmethod
        def decode_record_batch_py(cls, _metadata: object, _batch: object) -> object:
            """
            Decode record batch py.
            """
            return []

    with pytest.raises(TypeError, match="from_json"):
        register_custom_data_class(MissingFromJson)
