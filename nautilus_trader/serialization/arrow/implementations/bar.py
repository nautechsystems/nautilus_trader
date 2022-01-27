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

from typing import Dict

from nautilus_trader.model.data.bar import Bar
from nautilus_trader.serialization.arrow.serializer import register_parquet


def serialize(bar: Bar):
    data = bar.to_dict(bar)
    data["instrument_id"] = bar.type.instrument_id.value
    return data


def deserialize(data: Dict) -> Bar:
    ignore = ("instrument_id",)
    bar = Bar.from_dict({k: v for k, v in data.items() if k not in ignore})
    return bar


register_parquet(
    Bar,
    serializer=serialize,
    deserializer=deserialize,
)
