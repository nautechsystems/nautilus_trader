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
Acceptance tests that run the notebook-style guides published in the documentation.

Each guide runs in its own subprocess because a process supports only one engine or node
at a time. `NAUTILUS_DATA_DIR` points at an empty directory so the guides take the
bundled sample data path rather than any archives the developer happens to have
downloaded.

"""

from __future__ import annotations

import io
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest

from nautilus_trader.testkit import providers


REPO_ROOT = Path(__file__).resolve().parents[3]

GUIDES = [
    "docs/getting_started/quickstart.py",
    "docs/getting_started/backtest_low_level.py",
    "docs/getting_started/backtest_high_level.py",
    "docs/how_to/loading_external_data.py",
    "docs/tutorials/backtest_fx_bars.py",
    "docs/tutorials/backtest_orderbook_binance.py",
    "docs/tutorials/backtest_orderbook_bybit.py",
]


@pytest.mark.parametrize("guide", GUIDES)
def test_documentation_guide_runs(guide: str, tmp_path: Path) -> None:
    """
    Test the published guide runs to completion against the current API.
    """
    script = REPO_ROOT / guide
    empty_data_dir = tmp_path / "data"
    empty_data_dir.mkdir()

    result = subprocess.run(
        [sys.executable, str(script)],
        cwd=tmp_path,
        env={**os.environ, "NAUTILUS_DATA_DIR": str(empty_data_dir)},
        capture_output=True,
        text=True,
        timeout=600,
        check=False,
    )

    assert result.returncode == 0, f"{guide} failed:\n{result.stdout}\n{result.stderr}"


def test_sample_data_path_materializes_outside_a_source_checkout(
    monkeypatch: pytest.MonkeyPatch,
    tmp_path: Path,
) -> None:
    """
    Test the order book guides reach their sample archive from an installed wheel.
    """
    monkeypatch.syspath_prepend(str(REPO_ROOT / "docs" / "tutorials"))
    import orderbook_data

    name = "bybit/xrpusdt-ob500.data.zip"
    checkout_root = providers.TEST_DATA_DIR
    expected = (checkout_root / name).read_bytes()

    def fake_urlopen(url: str) -> io.BytesIO:
        return io.BytesIO((checkout_root / url.split("/test_data/", 1)[1]).read_bytes())

    missing_root = Path("/nonexistent/test_data")
    monkeypatch.setattr(orderbook_data, "TEST_DATA_DIR", missing_root)
    monkeypatch.setattr("urllib.request.urlopen", fake_urlopen)
    monkeypatch.setattr(tempfile, "gettempdir", lambda: str(tmp_path))

    with monkeypatch.context() as remote_only:
        remote_only.setattr(providers, "TEST_DATA_DIR", missing_root)
        materialized = orderbook_data.sample_data_path(name)

    assert materialized == tmp_path / "nautilus_sample_data" / name
    assert materialized.read_bytes() == expected
