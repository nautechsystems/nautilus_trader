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

from dataclasses import dataclass
from typing import Any

import msgspec
import pyarrow as pa

from nautilus_trader.core.datetime import unix_nanos_to_dt
from nautilus_trader.core.datetime import unix_nanos_to_iso8601
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.serialization.arrow.serializer import register_arrow
from nautilus_trader.serialization.base import register_serializable_type


def customdataclass(*args, **kwargs):  # noqa: C901 (too complex)
    def wrapper(cls):  # noqa: C901 (too complex)
        create_init = False
        create_repr = False

        if cls.__init__ is object.__init__:
            create_init = True

        if cls.__repr__ is object.__repr__:
            create_repr = True

        cls = dataclass(cls, **kwargs)

        if create_init:
            # cls.fields_init allows to use positional arguments for parameters other than ts_event and ts_init
            cls.fields_init = cls.__init__

            def __init__(self, ts_event: int = 0, ts_init: int = 0, *args2, **kwargs2):
                self.fields_init(*args2, **kwargs2)

                self._ts_event = ts_event
                self._ts_init = ts_init

            cls.__init__ = __init__

        if create_repr:
            cls.fields_repr = cls.__repr__

            def __repr__(self):
                repr = self.fields_repr()
                time_repr = (
                    f", ts_event={unix_nanos_to_iso8601(self._ts_event)}, "
                    + f"ts_init={unix_nanos_to_iso8601(self._ts_init)})"
                )

                return repr[:-1] + time_repr

            cls.__repr__ = __repr__

        if "ts_event" not in cls.__dict__:

            @property
            def ts_event(self) -> int:
                return self._ts_event

            cls.ts_event = ts_event

        if "ts_init" not in cls.__dict__:

            @property
            def ts_init(self) -> int:
                return self._ts_init

            cls.ts_init = ts_init

        if "to_dict" not in cls.__dict__:

            def to_dict(self, to_arrow=False) -> dict[str, Any]:
                result = {attr: getattr(self, attr) for attr in self.__annotations__}

                if hasattr(self, "instrument_id"):
                    result["instrument_id"] = self.instrument_id.value

                result["type"] = str(cls.__name__)
                result["ts_event"] = self._ts_event
                result["ts_init"] = self._ts_init

                if to_arrow:
                    result["date"] = int(unix_nanos_to_dt(result["ts_event"]).strftime("%Y%m%d"))

                return result

            cls.to_dict = to_dict

        if "from_dict" not in cls.__dict__:

            @classmethod
            def from_dict(cls, data: dict[str, Any]) -> cls:
                data.pop("type", None)
                data.pop("date", None)

                if "instrument_id" in data:
                    data["instrument_id"] = InstrumentId.from_str(data["instrument_id"])

                return cls(**data)

            cls.from_dict = from_dict

        if "to_bytes" not in cls.__dict__:

            def to_bytes(self) -> bytes:
                return msgspec.msgpack.encode(self.to_dict())

            cls.to_bytes = to_bytes

        if "from_bytes" not in cls.__dict__:

            @classmethod
            def from_bytes(cls, data: bytes) -> cls:
                return cls.from_dict(msgspec.msgpack.decode(data))

            cls.from_bytes = from_bytes

        if "to_arrow" not in cls.__dict__:

            def to_arrow(self) -> pa.RecordBatch:
                return pa.RecordBatch.from_pylist([self.to_dict(to_arrow=True)], schema=cls._schema)

            cls.to_arrow = to_arrow

        if "from_arrow" not in cls.__dict__:

            @classmethod
            def from_arrow(cls, table: pa.Table) -> cls:
                return [cls.from_dict(d) for d in table.to_pylist()]

            cls.from_arrow = from_arrow

        if "_schema" not in cls.__dict__:
            type_mapping = {
                "InstrumentId": pa.string(),
                "str": pa.string(),
                "bool": pa.bool_(),
                "float": pa.float64(),
                "int": pa.int64(),
                "bytes": pa.binary(),
                "ndarray": pa.binary(),
            }

            cls._schema = pa.schema(
                {
                    attr: type_mapping[cls.__annotations__[attr].__name__]
                    for attr in cls.__annotations__
                }
                | {
                    "type": pa.string(),
                    "ts_event": pa.int64(),
                    "ts_init": pa.int64(),
                    "date": pa.int32(),
                },
            )

        register_serializable_type(cls, cls.to_dict, cls.from_dict)
        register_arrow(cls, cls._schema, cls.to_arrow, cls.from_arrow)

        return cls

    if args and callable(args[0]):
        return wrapper(args[0])

    return wrapper
