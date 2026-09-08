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
"""
Parquet catalog regression tests.
"""

import json
from pathlib import Path
from typing import Any

from nautilus_trader.common import Cache
from nautilus_trader.common import Clock
from nautilus_trader.model import GreeksConvention
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import OptionGreeks
from nautilus_trader.persistence import ParquetDataCatalog
from nautilus_trader.persistence import StreamingFeatherWriter


def _writer(path: Path, **kwargs: Any) -> StreamingFeatherWriter:
    path.mkdir(parents=True)
    return StreamingFeatherWriter(
        path=str(path),
        cache=Cache(),
        clock=Clock.new_test(),
        **kwargs,
    )


def test_streaming_feather_writer_option_greeks_marks_run_non_empty(tmp_path: Path) -> None:
    """
    Verify streaming feather writer option greeks marks run non empty.
    """
    path = tmp_path / "backtest" / "run-greeks"
    writer = _writer(path)
    greeks = OptionGreeks(
        InstrumentId.from_str("BTC-20260529-100000-C.OKX"),
        0.55,
        0.012,
        3.4,
        -1.2,
        0.01,
        0.64,
        None,
        0.66,
        100_000.0,
        None,
        90,
        91,
        GreeksConvention.PRICE_ADJUSTED,
    )

    writer.write(greeks)
    writer.flush()

    manifest_path = path / "_nautilus_run_manifest.json"
    manifest = json.loads(manifest_path.read_text())
    assert {
        key: manifest[key] for key in ["schema_version", "kind", "instance_id", "status", "empty"]
    } == {
        "schema_version": 1,
        "kind": "backtest",
        "instance_id": "run-greeks",
        "status": "in_progress",
        "empty": False,
    }
    writer.close()
    manifest = json.loads(manifest_path.read_text())
    assert {
        key: manifest[key] for key in ["schema_version", "kind", "instance_id", "status", "empty"]
    } == {
        "schema_version": 1,
        "kind": "backtest",
        "instance_id": "run-greeks",
        "status": "completed",
        "empty": False,
    }
    assert ParquetDataCatalog(str(tmp_path)).list_backtest_runs() == ["run-greeks"]
