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

import msgspec
import pyarrow as pa

from nautilus_trader.common.config import msgspec_encoding_hook
from nautilus_trader.model.data import FundingRateUpdate
from nautilus_trader.serialization.arrow.schema import NAUTILUS_ARROW_SCHEMA


def serialize(funding_rate: FundingRateUpdate) -> pa.RecordBatch:
    data = funding_rate.to_dict(funding_rate)
    data["rate"] = msgspec.json.encode(data["rate"], enc_hook=msgspec_encoding_hook)
    schema = NAUTILUS_ARROW_SCHEMA[FundingRateUpdate].with_metadata(
        {"instrument_id": funding_rate.instrument_id.value},
    )
    return pa.RecordBatch.from_pylist([data], schema=schema)


def deserialize(batch: pa.RecordBatch) -> list[FundingRateUpdate]:
    def parse(data):
        data["instrument_id"] = batch.schema.metadata[b"instrument_id"].decode()
        data["rate"] = msgspec.json.decode(data["rate"])
        return data

    return [FundingRateUpdate.from_dict(parse(d)) for d in batch.to_pylist()]
