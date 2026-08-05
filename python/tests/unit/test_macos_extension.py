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

import importlib
import subprocess
import sys
import textwrap
from pathlib import Path

import pytest


pytestmark = pytest.mark.skipif(sys.platform != "darwin", reason="macOS-specific extension tests")


def test_extension_exports_only_module_initializer() -> None:
    libnautilus = importlib.import_module("nautilus_trader._libnautilus")
    extension_path = Path(libnautilus.__file__)

    result = subprocess.run(
        ["/usr/bin/nm", "-gjU", str(extension_path)],
        capture_output=True,
        check=False,
        text=True,
    )

    assert result.returncode == 0, result.stderr
    assert result.stdout.splitlines() == ["_PyInit__libnautilus"]


@pytest.mark.parametrize(
    ("script", "expected_stdout"),
    [
        pytest.param(
            """
            import pyarrow
            import nautilus_trader

            print("reached end", flush=True)
            """,
            "reached end\n",
            id="pyarrow-before-nautilus",
        ),
        pytest.param(
            """
            import nautilus_trader
            import pandas

            from nautilus_trader.model import Currency

            Currency.from_str("USDC")
            print("currency constructed", flush=True)
            """,
            "currency constructed\n",
            id="currency-after-pandas",
        ),
        pytest.param(
            """
            import tempfile

            from nautilus_trader.persistence import ParquetDataCatalog
            import pandas

            with tempfile.TemporaryDirectory() as directory:
                ParquetDataCatalog(directory)
            print("catalog constructed", flush=True)
            """,
            "catalog constructed\n",
            id="catalog-with-pandas",
        ),
    ],
)
def test_reported_pyarrow_reproductions_in_fresh_process(
    script: str,
    expected_stdout: str,
) -> None:
    pytest.importorskip("pandas")
    pytest.importorskip("pyarrow")

    result = subprocess.run(
        [sys.executable, "-c", textwrap.dedent(script)],
        capture_output=True,
        check=False,
        text=True,
    )

    assert result.returncode == 0, (result.stdout, result.stderr)
    assert result.stdout == expected_stdout
