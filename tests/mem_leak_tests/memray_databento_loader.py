#!/usr/bin/env python3
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

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.identifiers import InstrumentId
from tests.integration_tests.adapters.databento.test_loaders import DATABENTO_TEST_DATA_DIR


if __name__ == "__main__":
    loader = DatabentoDataLoader()
    path = DATABENTO_TEST_DATA_DIR / "temp" / "tsla-xnas-20240107-20240206.trades.dbn.zst"
    instrument_id = InstrumentId.from_str("TSLA.XNAS")

    count = 0
    total_runs = 128
    while count < total_runs:
        count += 1
        print(f"Run: {count}/{total_runs}")

        data = loader.from_dbn_file(path, instrument_id=instrument_id, as_legacy_cython=True)
        assert len(data) == 6_885_435
