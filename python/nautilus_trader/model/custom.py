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
from dataclasses import asdict
from dataclasses import dataclass
from inspect import get_annotations
from typing import Any
from typing import dataclass_transform
from typing import get_origin

from nautilus_trader.model import InstrumentId


@dataclass_transform()
def customdataclass(*args, **kwargs):  # noqa: C901 (too complex)
    """
    Add the interface required for Python custom data backed by PyO3.

    Register the decorated class with ``register_custom_data_class`` before wrapping instances in
    ``CustomData``. Arrow is imported only when catalog encode or decode methods are used.

    """

    def wrapper(cls):  # noqa: C901 (too complex)
        cls = dataclass(cls, **kwargs)
        fields_init = cls.__init__

        def __init__(self, ts_event: int = 0, ts_init: int = 0, *field_args, **field_kwargs):
            fields_init(self, *field_args, **field_kwargs)
            self._ts_event = ts_event
            self._ts_init = ts_init

        cls.__init__ = __init__

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
                values = asdict(self)
                for name, annotation in _get_annotations(self.__class__).items():
                    values[name] = _serialize_field_value(annotation, values[name])
                values["type"] = self.__class__.__name__
                values["ts_event"] = self.ts_event
                values["ts_init"] = self.ts_init
                return values

            cls.to_dict = to_dict

        if "from_dict" not in cls.__dict__:

            @classmethod
            def from_dict(cls_inner, values: dict[str, Any]):
                values = dict(values)
                values.pop("type", None)
                values.pop("data_type", None)
                for name, annotation in _get_annotations(cls_inner).items():
                    if name in values:
                        values[name] = _deserialize_field_value(annotation, values[name])
                return cls_inner(**values)

            cls.from_dict = from_dict

        if "to_json" not in cls.__dict__:

            def to_json(self) -> str:
                return json.dumps(self.to_dict())

            cls.to_json = to_json

        if "from_json" not in cls.__dict__:

            @classmethod
            def from_json(cls_inner, data: bytes | str | dict[str, Any]):
                values = json.loads(data) if isinstance(data, (bytes, str)) else data
                return cls_inner.from_dict(values)

            cls.from_json = from_json

        if "type_name_static" not in cls.__dict__:

            @classmethod
            def type_name_static(cls_inner) -> str:
                return cls_inner.__name__

            cls.type_name_static = type_name_static

        if "arrow_schema_py" not in cls.__dict__:

            @classmethod
            def arrow_schema_py(cls_inner):
                import pyarrow as pa

                return _arrow_schema_for_class(cls_inner, pa)

            cls.arrow_schema_py = arrow_schema_py

        if "encode_record_batch_py" not in cls.__dict__:

            def encode_record_batch_py(self, items: list):
                import pyarrow as pa

                return pa.RecordBatch.from_pylist(
                    [item.to_dict() for item in items],
                    schema=_arrow_schema_for_class(self.__class__, pa),
                )

            cls.encode_record_batch_py = encode_record_batch_py

        if "decode_record_batch_py" not in cls.__dict__:

            @classmethod
            def decode_record_batch_py(cls_inner, metadata: dict, batch) -> list:
                fields = {*_get_annotations(cls_inner), "type", "ts_event", "ts_init"}
                values = [
                    {name: value for name, value in row.items() if name in fields}
                    for row in batch.to_pylist()
                ]

                for name in ("ts_event", "ts_init"):
                    index = batch.schema.get_field_index(name)
                    if index < 0:
                        continue
                    timestamps = batch.column(index).cast("int64").to_pylist()
                    for row, timestamp in zip(values, timestamps, strict=True):
                        row[name] = timestamp
                return [cls_inner.from_dict(row) for row in values]

            cls.decode_record_batch_py = decode_record_batch_py

        return cls

    if args and callable(args[0]):
        return wrapper(args[0])

    return wrapper


def _get_annotations(cls) -> dict[str, Any]:
    return get_annotations(cls, eval_str=True)


def _serialize_field_value(annotation: Any, value: Any) -> Any:
    if annotation is InstrumentId and value is not None:
        return str(value)
    if _is_dict_annotation(annotation) and value is not None:
        return json.dumps(value, sort_keys=True)
    return value


def _deserialize_field_value(annotation: Any, value: Any) -> Any:
    if annotation is InstrumentId and isinstance(value, str):
        return InstrumentId.from_str(value)
    if _is_dict_annotation(annotation) and isinstance(value, str):
        return json.loads(value)
    return value


def _is_dict_annotation(annotation: Any) -> bool:
    return annotation is dict or get_origin(annotation) is dict


def _arrow_schema_for_class(cls, pa):
    type_mapping = {
        InstrumentId: pa.string(),
        str: pa.string(),
        bool: pa.bool_(),
        float: pa.float64(),
        int: pa.int64(),
        bytes: pa.binary(),
        dict: pa.string(),
    }
    fields = [
        pa.field(name, type_mapping[get_origin(annotation) or annotation])
        for name, annotation in _get_annotations(cls).items()
    ]
    fields.extend(
        [
            pa.field("type", pa.string(), nullable=False),
            pa.field("ts_event", pa.timestamp("ns", tz="UTC"), nullable=False),
            pa.field("ts_init", pa.timestamp("ns", tz="UTC"), nullable=False),
        ],
    )
    return pa.schema(fields)
