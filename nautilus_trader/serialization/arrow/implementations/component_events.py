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

from nautilus_trader.common.messages import ComponentStateChanged
from nautilus_trader.common.messages import TradingStateChanged
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(event: ComponentStateChanged | TradingStateChanged) -> pa.RecordBatch:
    data = event.to_dict(event)
    data["config"] = msgspec.json.encode(data["config"])
    return pa.RecordBatch.from_pylist([data], schema=NAUTILUS_ARROW_SCHEMA[type(event)])


def deserialize(cls):
    def inner(batch: pa.RecordBatch) -> list[ComponentStateChanged | TradingStateChanged]:
        def parse(data):
            data["config"] = msgspec.json.decode(data["config"])
            return data

        return [cls.from_dict(parse(d)) for d in batch.to_pylist()]

    return inner
