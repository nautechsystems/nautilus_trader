# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

import pyarrow as pa
import pyarrow.parquet as pq

from nautilus_trader.serialization.arrow.schema import SCHEMA_TO_TYPE
from nautilus_trader.serialization.arrow.schema import TYPE_TO_SCHEMA
from nautilus_trader.serialization.arrow.transformer import deserialize
from nautilus_trader.serialization.arrow.transformer import serialize
from nautilus_trader.serialization.arrow.util import list_dicts_to_dict_lists
from nautilus_trader.serialization.arrow.util import maybe_list


class ArrowSerializer:
    """
    Provides a serializer for the Arrow / Parquet specification.
    """

    @staticmethod
    def to_parquet(buff, objects: list):
        schema = TYPE_TO_SCHEMA[objects[0].__class__]
        mapping = list_dicts_to_dict_lists(
            [x for obj in objects for x in maybe_list(serialize(obj))]
        )
        table = pa.Table.from_pydict(mapping=mapping, schema=schema)

        with pq.ParquetWriter(buff, schema) as writer:
            writer.write_table(table)
        return buff

    @staticmethod
    def from_parquet(message_bytes, **kwargs):
        metadata = pq.read_metadata(message_bytes)
        cls = SCHEMA_TO_TYPE[metadata.metadata[b"type"]]
        table = pq.read_table(message_bytes, **kwargs)
        data = table.to_pydict()
        values = list(
            map(dict, zip(*([(key, val) for val in data[key]] for key in data.keys())))
        )
        return deserialize(cls=cls, data=values)

    # TODO (bm) - Implement for IPC / streaming. See https://arrow.apache.org/docs/python/ipc.html
    # @staticmethod
    # def to_arrow(message: bytes):
    #     """
    #     Serialize the given message to `MessagePack` specification bytes.
    #
    #     Parameters
    #     ----------
    #     message : dict
    #         The message to serialize.
    #
    #     Returns
    #     -------
    #     bytes
    #
    #     """
    #     Condition.not_none(message, "message")
    #
    #     batch = pa.record_batch(data, names=['f0', 'f1', 'f2'])
    #
    #     sink = pa.BufferOutputStream()
    #
    #     writer = pa.ipc.new_stream(sink, batch.schema)
    #     return msgpack.packb(message)
    #
    # @staticmethod
    # def from_arrow(message_bytes: bytes):
    #     """
    #     Deserialize the given `MessagePack` specification bytes to a dictionary.
    #
    #     Parameters
    #     ----------
    #     message_bytes : bytes
    #         The message bytes to deserialize.
    #
    #     Returns
    #     -------
    #     dict[str, object]
    #
    #     """
    #     Condition.not_none(message_bytes, "message_bytes")
    #
    #     return msgpack.unpackb(message_bytes)
