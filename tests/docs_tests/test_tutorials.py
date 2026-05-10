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
import os
import subprocess
import sys
import tempfile
from pathlib import Path

import pytest


REPO_ROOT = Path(__file__).resolve().parents[2]
GETTING_STARTED = REPO_ROOT / "docs" / "getting_started"
TUTORIALS = REPO_ROOT / "docs" / "tutorials"
HOW_TO = REPO_ROOT / "docs" / "how_to"

# Tutorials that use bundled test data and can execute without external dependencies
EXECUTABLE_TUTORIALS = [
    GETTING_STARTED / "quickstart.py",
    GETTING_STARTED / "backtest_low_level.py",
    TUTORIALS / "backtest_fx_bars.py",
]

CI_SKIPPED_EXECUTABLE_TUTORIALS = {
    GETTING_STARTED / "backtest_low_level.py",
    TUTORIALS / "backtest_fx_bars.py",
}

LOCAL_DATA = REPO_ROOT / "tests" / "test_data" / "local"

# Tutorials that run when user-fetched data exists under tests/test_data/local/.
# Each entry is (script, data_subdir) where data_subdir is the subdirectory under
# LOCAL_DATA that must contain files for the tutorial to run.
LOCAL_DATA_TUTORIALS = [
    (GETTING_STARTED / "backtest_high_level.py", "HISTDATA"),
    (TUTORIALS / "backtest_orderbook_binance.py", "Binance"),
    (TUTORIALS / "backtest_orderbook_bybit.py", "Bybit"),
    (HOW_TO / "loading_external_data.py", "HISTDATA"),
]

# Tutorials that need API keys or network access and cannot run locally
NON_EXECUTABLE_TUTORIALS = [
    HOW_TO / "data_catalog_databento.py",
]

ALL_TUTORIALS = (
    EXECUTABLE_TUTORIALS + [t for t, _ in LOCAL_DATA_TUTORIALS] + NON_EXECUTABLE_TUTORIALS
)


def _tutorial_id(path: Path) -> str:
    return f"{path.parent.name}/{path.name}"


@pytest.mark.parametrize("tutorial", EXECUTABLE_TUTORIALS, ids=_tutorial_id)
def test_tutorial_executes(tutorial: Path) -> None:
    if os.getenv("CI") == "true" and tutorial in CI_SKIPPED_EXECUTABLE_TUTORIALS:
        pytest.skip(
            "Uses TestDataProvider fallback, which depends on the GitHub API when CI runs from an installed wheel.",
        )

    with tempfile.TemporaryDirectory() as tmpdir:
        result = subprocess.run(  # noqa: S603
            [sys.executable, str(tutorial)],
            capture_output=True,
            text=True,
            timeout=120,
            cwd=tmpdir,
            check=False,
        )
    assert result.returncode == 0, (
        f"{tutorial.name} failed with exit code {result.returncode}\nstderr:\n{result.stderr}"
    )


@pytest.mark.parametrize(
    ("tutorial", "data_subdir"),
    LOCAL_DATA_TUTORIALS,
    ids=[_tutorial_id(t) for t, _ in LOCAL_DATA_TUTORIALS],
)
def test_tutorial_with_local_data(tutorial: Path, data_subdir: str) -> None:
    data_dir = LOCAL_DATA / data_subdir
    if not data_dir.exists() or not any(data_dir.iterdir()):
        pytest.skip(f"User-fetched test data not found: {data_dir}")

    env = os.environ.copy()
    env["NAUTILUS_DATA_DIR"] = str(LOCAL_DATA)

    with tempfile.TemporaryDirectory() as tmpdir:
        result = subprocess.run(  # noqa: S603
            [sys.executable, str(tutorial)],
            capture_output=True,
            text=True,
            timeout=300,
            cwd=tmpdir,
            env=env,
            check=False,
        )
    assert result.returncode == 0, (
        f"{tutorial.name} failed with exit code {result.returncode}\nstderr:\n{result.stderr}"
    )
    assert "[ERROR]" not in result.stderr, (
        f"{tutorial.name} logged errors during execution:\n{result.stderr}"
    )


@pytest.mark.parametrize("tutorial", ALL_TUTORIALS, ids=_tutorial_id)
def test_tutorial_syntax(tutorial: Path) -> None:
    source = tutorial.read_text()
    ast.parse(source, filename=str(tutorial))
