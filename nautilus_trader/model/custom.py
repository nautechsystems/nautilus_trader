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
import sys
from dataclasses import dataclass
from typing import Any

import msgspec
import pyarrow as pa

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
                has_fields = not repr.endswith("()")

                time_repr = (
                    f"{', ' if has_fields else ''}ts_event={unix_nanos_to_iso8601(self._ts_event)}, "
                    f"ts_init={unix_nanos_to_iso8601(self._ts_init)})"
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

            def to_dict(self) -> dict[str, Any]:
                # Python 3.14+ uses PEP 649 lazy annotations
                if (
                    sys.version_info >= (3, 14)
                    and hasattr(self.__class__, "__annotate__")
                    and self.__class__.__annotate__
                ):
                    annotations = self.__class__.__annotate__(1)  # 1 = eval annotations
                else:
                    annotations = getattr(self.__class__, "__annotations__", {})

                result = {attr: getattr(self, attr) for attr in annotations}

                if hasattr(self, "instrument_id"):
                    result["instrument_id"] = self.instrument_id.value

                result["type"] = str(cls.__name__)
                result["ts_event"] = self._ts_event
                result["ts_init"] = self._ts_init

                return result

            cls.to_dict = to_dict

        if "from_dict" not in cls.__dict__:

            @classmethod
            def from_dict(cls, data: dict[str, Any]) -> cls:
                data.pop("type", None)
                data.pop("data_type", None)

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
                return pa.RecordBatch.from_pylist([self.to_dict()], schema=cls._schema)

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
                },
            )

        register_serializable_type(cls, cls.to_dict, cls.from_dict)
        register_arrow(cls, cls._schema, cls.to_arrow, cls.from_arrow)

        return cls

    if args and callable(args[0]):
        return wrapper(args[0])

    return wrapper


def customdataclass_pyo3(*args, **kwargs):  # noqa: C901 (too complex)
    """
    Extend customdataclass with methods required for the PyO3 (Rust) catalog.

    Use this when you want to write/query custom data via ParquetDataCatalogV2
    (nautilus_pyo3). After defining your class, register it once by type:

        from nautilus_trader.core.nautilus_pyo3.model import register_custom_data_class
        register_custom_data_class(MyClass)

    Then use catalog.write_custom_data([...]) and catalog.query("MyClass", ...).

    """

    def wrapper(cls):  # noqa: C901 (too complex)
        cls = customdataclass(*args, **kwargs)(cls)

        if "to_json" not in cls.__dict__:

            def to_json(self) -> str:
                return json.dumps(self.to_dict())

            cls.to_json = to_json

        if "from_json" not in cls.__dict__:

            @classmethod
            def from_json(cls_inner, data: dict[str, Any]) -> Any:
                return cls_inner.from_dict(data)

            cls.from_json = from_json

        if "type_name_static" not in cls.__dict__:

            @classmethod
            def type_name_static(cls_inner) -> str:
                return cls_inner.__name__

            cls.type_name_static = type_name_static

        if "encode_record_batch_py" not in cls.__dict__:

            def encode_record_batch_py(self, items: list) -> pa.RecordBatch:
                if not hasattr(self.__class__, "_schema"):
                    msg = (
                        f"{self.__class__.__name__}: _schema not set. "
                        "Register the type with register_custom_data_class(...) so the "
                        "catalog can encode record batches."
                    )
                    raise AttributeError(msg)
                dicts = [x.to_dict() for x in items]
                return pa.RecordBatch.from_pylist(dicts, schema=self.__class__._schema)

            cls.encode_record_batch_py = encode_record_batch_py

        if "decode_record_batch_py" not in cls.__dict__:

            @classmethod
            def decode_record_batch_py(cls_inner, metadata: dict, batch: pa.RecordBatch) -> list:
                return [cls_inner.from_dict(d) for d in batch.to_pylist()]

            cls.decode_record_batch_py = decode_record_batch_py

        return cls

    if args and callable(args[0]):
        return wrapper(args[0])

    return wrapper
