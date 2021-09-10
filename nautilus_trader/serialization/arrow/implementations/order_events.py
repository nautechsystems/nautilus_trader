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

from typing import Dict, List

import orjson

from nautilus_trader.model.events.order import OrderEvent
from nautilus_trader.model.events.order import OrderInitialized


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
    data.update({"trigger": False, "price": None})
    data.update(orjson.loads(data.pop("options", "{}")))  # noqa: P103
    data = {k: caster[k](v) if (k in caster and v is not None) else v for k, v in data.items()}
    return data


def deserialize(data: List[Dict]):
    raise NotImplementedError()  # pragma: no cover
