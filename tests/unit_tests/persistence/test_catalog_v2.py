# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from io import BytesIO

import msgspec
import pyarrow as pa

from nautilus_trader.core.nautilus_pyo3.model import QuoteTick as RustQuoteTick
from nautilus_trader.core.nautilus_pyo3.persistence import DataBackendSession
from nautilus_trader.persistence.wranglers import QuoteTickDataWrangler
from nautilus_trader.test_kit.providers import TestDataProvider
from nautilus_trader.test_kit.providers import TestInstrumentProvider


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")


def test_quote_ticks_to_record_batch_reader() -> None:
    # Arrange
    wrangler = QuoteTickDataWrangler(instrument=AUDUSD_SIM)

    provider = TestDataProvider()
    ticks = wrangler.process(provider.read_csv_ticks("truefx-audusd-ticks.csv"))

    ticks_pyo3 = []
    for tick in ticks:
        json_bytes = msgspec.json.encode(tick.to_dict(tick))
        ticks_pyo3.append(RustQuoteTick.from_json(json_bytes))

    session = DataBackendSession()

    # Act
    batches_bytes = session.quote_ticks_to_batches_bytes(ticks_pyo3)
    batches_stream = BytesIO(batches_bytes)
    reader = pa.ipc.open_stream(batches_stream)

    # Assert
    assert len(reader.read_all()) == len(ticks)
    assert len(ticks) == 100_000

    reader.close()
