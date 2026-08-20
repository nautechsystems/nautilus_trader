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

from pathlib import Path

from nautilus_trader.adapters.databento import DatabentoDataLoader
from nautilus_trader.adapters.databento import DatabentoImbalance
from nautilus_trader.adapters.databento import DatabentoStatistics
from nautilus_trader.model import InstrumentId


TEST_DATA_DIR = Path(__file__).resolve().parents[5] / "crates/adapters/databento/test_data"
PUBLISHERS_FILE = TEST_DATA_DIR.parent / "publishers.json"


def test_databento_imbalance_python_roundtrip() -> None:
    loader = DatabentoDataLoader(PUBLISHERS_FILE)
    loader.set_price_precision("SPOT", 2)
    data = loader.load_imbalance(TEST_DATA_DIR / "test_data.imbalance.dbn.zst")

    assert len(data) == 2
    for original in data:
        restored = DatabentoImbalance.from_dict(original.to_dict())

        assert restored.instrument_id == original.instrument_id
        assert restored.ref_price == original.ref_price
        assert restored.cont_book_clr_price == original.cont_book_clr_price
        assert restored.auct_interest_clr_price == original.auct_interest_clr_price
        assert restored.paired_qty == original.paired_qty
        assert restored.total_imbalance_qty == original.total_imbalance_qty
        assert restored.side == original.side
        assert restored.significant_imbalance == original.significant_imbalance
        assert restored.ts_event == original.ts_event
        assert restored.ts_recv == original.ts_recv
        assert restored.ts_init == original.ts_init
        assert restored == original


def test_databento_statistics_python_roundtrip() -> None:
    loader = DatabentoDataLoader(PUBLISHERS_FILE)
    loader.set_price_precision("ESM4", 2)
    instrument_id = InstrumentId.from_str("ESM4.GLBX")
    data = loader.load_statistics(
        TEST_DATA_DIR / "test_data.statistics.dbn.zst",
        instrument_id=instrument_id,
    )

    assert len(data) == 2
    for original in data:
        restored = DatabentoStatistics.from_dict(original.to_dict())

        assert restored.instrument_id == original.instrument_id
        assert restored.stat_type == original.stat_type
        assert restored.update_action == original.update_action
        assert restored.price == original.price
        assert restored.quantity == original.quantity
        assert restored.channel_id == original.channel_id
        assert restored.stat_flags == original.stat_flags
        assert restored.sequence == original.sequence
        assert restored.ts_ref == original.ts_ref
        assert restored.ts_in_delta == original.ts_in_delta
        assert restored.ts_event == original.ts_event
        assert restored.ts_recv == original.ts_recv
        assert restored.ts_init == original.ts_init
        assert restored == original
