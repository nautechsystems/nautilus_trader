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

import msgspec
import pyarrow as pa

from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.events import OrderInitialized
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(event: OrderInitialized | OrderFilled) -> pa.RecordBatch:
    data = event.to_dict(event)
    if isinstance(event, OrderInitialized):
        data["options"] = msgspec.json.encode(data["options"])
        data["linked_order_ids"] = msgspec.json.encode(data["linked_order_ids"])
        data["exec_algorithm_params"] = msgspec.json.encode(data["exec_algorithm_params"])
        data["tags"] = msgspec.json.encode(data["tags"])
    elif isinstance(event, OrderFilled):
        data["info"] = msgspec.json.encode(data["info"])
    return pa.RecordBatch.from_pylist([data], schema=NAUTILUS_ARROW_SCHEMA[type(event)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> OrderInitialized | OrderFilled:
        def parse(data):
            if cls == OrderInitialized:
                data["options"] = msgspec.json.decode(data["options"])
                data["linked_order_ids"] = msgspec.json.decode(data["linked_order_ids"])
                data["exec_algorithm_params"] = msgspec.json.decode(data["exec_algorithm_params"])
                data["tags"] = msgspec.json.decode(data["tags"])
            elif cls == OrderFilled:
                data["info"] = msgspec.json.decode(data["info"])
            else:
                raise RuntimeError("Unsupported order event type for deserialization")
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner
