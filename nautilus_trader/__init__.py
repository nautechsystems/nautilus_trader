# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
The top-level package contains all sub-packages needed for NautilusTrader.
"""

import tomllib
from pathlib import Path
from typing import Final


PACKAGE_ROOT: Final[Path] = Path(__file__).resolve().parent.parent
TEST_DATA_DIR: Final[Path] = PACKAGE_ROOT / "tests" / "test_data"

try:
    with open(PACKAGE_ROOT / "pyproject.toml", "rb") as f:
        pyproject_data = tomllib.load(f)
    __version__ = pyproject_data["tool"]["poetry"]["version"]
except FileNotFoundError:  # pragma: no cover
    __version__ = "latest"

USER_AGENT: Final[str] = f"NautilusTrader/{__version__}"
