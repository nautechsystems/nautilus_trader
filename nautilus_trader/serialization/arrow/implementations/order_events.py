# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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
from typing import Dict

import msgspec

from nautilus_trader.model.events.order import OrderEvent
from nautilus_trader.model.events.order import OrderFilled
from nautilus_trader.model.events.order import OrderInitialized
from nautilus_trader.serialization.arrow.schema import NAUTILUS_PARQUET_SCHEMA
from nautilus_trader.serialization.arrow.serializer import register_parquet


def serialize(event: OrderEvent):
    caster = {
        "last_qty": float,
        "last_px": float,
    }
    data = {k: caster[k](v) if k in caster else v for k, v in event.to_dict(event).items()}
    return data


def serialize_order_initialized(event: OrderInitialized):
    caster = {
        "quantity": float,
        "price": float,
    }
    data = event.to_dict(event)
    data.update(json.loads(data.pop("options", "{}")))  # noqa: P103
    data = {k: caster[k](v) if (k in caster and v is not None) else v for k, v in data.items()}
    return data


def deserialize_order_filled(data: Dict) -> OrderFilled:
    for k in ("last_px", "last_qty"):
        data[k] = str(data[k])
    return OrderFilled.from_dict(data)


def deserialize_order_initialised(data: Dict) -> OrderInitialized:
    for k in ("price", "quantity"):
        data[k] = str(data[k])
    options_fields = msgspec.json.decode(
        NAUTILUS_PARQUET_SCHEMA[OrderInitialized].metadata[b"options_fields"]
    )
    data["options"] = msgspec.json.encode({k: data.pop(k, None) for k in options_fields})
    return OrderInitialized.from_dict(data)


register_parquet(OrderFilled, serializer=serialize, deserializer=deserialize_order_filled)
register_parquet(
    OrderInitialized,
    serializer=serialize_order_initialized,
    deserializer=deserialize_order_initialised,
)
