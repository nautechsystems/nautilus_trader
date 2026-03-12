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

import ast
import subprocess
import sys
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
GETTING_STARTED = REPO_ROOT / "docs" / "getting_started"
TUTORIALS = REPO_ROOT / "docs" / "tutorials"

# Tutorials that use bundled test data and can execute without external dependencies
EXECUTABLE_TUTORIALS = [
    GETTING_STARTED / "quickstart.py",
    GETTING_STARTED / "backtest_low_level.py",
    TUTORIALS / "backtest_fx_bars.py",
]

# Tutorials that need external data, API keys, or network access
NON_EXECUTABLE_TUTORIALS = [
    GETTING_STARTED / "backtest_high_level.py",
    TUTORIALS / "backtest_binance_orderbook.py",
    TUTORIALS / "backtest_bybit_orderbook.py",
    TUTORIALS / "databento_data_catalog.py",
    TUTORIALS / "loading_external_data.py",
]

ALL_TUTORIALS = EXECUTABLE_TUTORIALS + NON_EXECUTABLE_TUTORIALS


def _tutorial_id(path: Path) -> str:
    return f"{path.parent.name}/{path.name}"


@pytest.mark.parametrize("tutorial", EXECUTABLE_TUTORIALS, ids=_tutorial_id)
def test_tutorial_executes(tutorial: Path) -> None:
    result = subprocess.run(  # noqa: S603
        [sys.executable, str(tutorial)],
        capture_output=True,
        text=True,
        timeout=120,
        cwd=REPO_ROOT,
        check=False,
    )
    assert result.returncode == 0, (
        f"{tutorial.name} failed with exit code {result.returncode}\nstderr:\n{result.stderr}"
    )


@pytest.mark.parametrize("tutorial", ALL_TUTORIALS, ids=_tutorial_id)
def test_tutorial_syntax(tutorial: Path) -> None:
    source = tutorial.read_text()
    ast.parse(source, filename=str(tutorial))
