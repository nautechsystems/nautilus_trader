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

import inspect
import json

import pytest

from nautilus_trader.model import CustomData
from nautilus_trader.model import DataType
from nautilus_trader.model import custom_data_backend_kind
from nautilus_trader.model import deserialize_custom_from_json
from nautilus_trader.model import drop_cvec_pycapsule
from nautilus_trader.model import register_custom_data_class


def test_data_type_construction():
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert dt.type_name == "QuoteTick"
    assert dt.metadata == {"instrument_id": "AUD/USD.SIM"}


def test_data_type_equality():
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt3 = DataType("QuoteTick", metadata={"instrument_id": "GBP/USD.SIM"})

    assert dt1 == dt2
    assert dt1 != dt3


def test_data_type_hash():
    dt1 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})
    dt2 = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert hash(dt1) == hash(dt2)


def test_data_type_topic():
    dt = DataType("QuoteTick", metadata={"instrument_id": "AUD/USD.SIM"})

    assert "QuoteTick" in dt.topic
    assert "AUD/USD.SIM" in dt.topic


def test_data_type_identifier():
    dt = DataType("QuoteTick", identifier="alpha")

    assert dt.identifier == "alpha"


def test_custom_data_python_backend_and_json_bytes():
    class Dummy:
        ts_event = 1
        ts_init = 2

        def __repr__(self):
            return "Dummy()"

    custom = CustomData(DataType("Example"), Dummy())
    payload = custom.to_json_bytes()

    assert custom.data_type.type_name == "Example"
    assert custom.ts_event == 1
    assert custom.ts_init == 2
    assert custom_data_backend_kind(custom) == "python"
    assert b'"type":"Dummy"' in payload


def test_register_custom_data_class_accepts_surface_compatible_class():
    class SurfaceCustomData:
        @classmethod
        def type_name_static(cls) -> str:
            return "SurfaceCustomData"

        @classmethod
        def from_json(cls, data):
            return cls()

        @classmethod
        def decode_record_batch_py(cls, metadata, batch):
            return []

    assert register_custom_data_class(SurfaceCustomData) is None


def test_deserialize_custom_from_json():
    class SurfaceCustomDataJson:
        def __init__(self, value=0, ts_event=0, ts_init=0):
            self.value = value
            self.ts_event = ts_event
            self.ts_init = ts_init

        @classmethod
        def type_name_static(cls) -> str:
            return "SurfaceCustomDataJson"

        @classmethod
        def from_json(cls, data):
            return cls(
                value=data.get("value", 0),
                ts_event=data.get("ts_event", 0),
                ts_init=data.get("ts_init", 0),
            )

        @classmethod
        def decode_record_batch_py(cls, metadata, batch):
            return []

    register_custom_data_class(SurfaceCustomDataJson)

    payload = json.dumps(
        {
            "type": "CustomData",
            "data_type": {"type": "SurfaceCustomDataJson"},
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


def test_register_custom_data_class_requires_decoder():
    class MissingDecoder:
        @classmethod
        def from_json(cls, data):
            return cls()

    with pytest.raises(TypeError, match="decode_record_batch_py"):
        register_custom_data_class(MissingDecoder)


def test_register_custom_data_class_requires_from_json():
    class MissingFromJson:
        @classmethod
        def decode_record_batch_py(cls, metadata, batch):
            return []

    with pytest.raises(TypeError, match="from_json"):
        register_custom_data_class(MissingFromJson)


def test_drop_cvec_pycapsule_signature_accepts_capsule_object():
    signature = inspect.signature(drop_cvec_pycapsule)
    parameter = signature.parameters["capsule"]

    assert list(signature.parameters) == ["capsule"]
    assert parameter.default is inspect.Signature.empty
