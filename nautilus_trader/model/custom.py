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

from typing import Any

import msgspec
import pyarrow as pa

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.base import register_serializable_type


def customdataclass(cls):  # noqa: C901 (too complex)
    if cls.__init__ is object.__init__:

        def __init__(self, ts_event: int, ts_init: int, **kwargs):
            for key, value in kwargs.items():
                setattr(self, key, value)

            self._ts_event = ts_event
            self._ts_init = ts_init

        cls.__init__ = __init__

    @property
    def ts_event(self) -> int:
        return self._ts_event

    cls.ts_event = ts_event

    @property
    def ts_init(self) -> int:
        return self._ts_init

    cls.ts_init = ts_init

    if not hasattr(cls, "to_dict"):

        def to_dict(self):
            result = {attr: getattr(self, attr) for attr in self.__annotations__}

            if hasattr(self, "instrument_id"):
                result["instrument_id"] = self.instrument_id.value
                result["ts_event"] = self._ts_event
                result["ts_init"] = self._ts_init

            return result

        cls.to_dict = to_dict

    if not hasattr(cls, "from_dict"):

        @classmethod
        def from_dict(cls, data: dict[str, Any]) -> cls:
            if "instrument_id" in data:
                data["instrument_id"] = InstrumentId.from_str(data["instrument_id"])

            return cls(**data)

        cls.from_dict = from_dict

    if not hasattr(cls, "to_bytes"):

        def to_bytes(self) -> bytes:
            return msgspec.msgpack.encode(self.to_dict())

        cls.to_bytes = to_bytes

    if not hasattr(cls, "from_bytes"):

        @classmethod
        def from_bytes(cls, data: bytes) -> cls:
            return cls.from_dict(msgspec.msgpack.decode(data))

        cls.from_bytes = from_bytes

    if not hasattr(cls, "to_arrow"):

        def to_arrow(self) -> pa.RecordBatch:
            return pa.RecordBatch.from_pylist([self.to_dict()], schema=cls._schema)

        cls.to_arrow = to_arrow

    if not hasattr(cls, "from_arrow"):

        @classmethod
        def from_arrow(cls, table: pa.Table) -> cls:
            return [cls.from_dict(d) for d in table.to_pylist()]

        cls.from_arrow = from_arrow

    if not hasattr(cls, "_schema"):
        type_mapping = {
            "InstrumentId": pa.string(),
            "bool": pa.bool_(),
            "float": pa.float64(),
            "int": pa.int64(),
        }

        cls._schema = pa.schema(
            {
                attr: type_mapping[cls.__annotations__[attr].__name__]
                for attr in cls.__annotations__
            },
        )

    register_serializable_type(cls, cls.to_dict, cls.from_dict)
    register_arrow(cls, cls._schema, cls.to_arrow, cls.from_arrow)

    return cls
