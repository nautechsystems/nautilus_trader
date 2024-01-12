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

from importlib import resources
from pathlib import Path

import toml


PACKAGE_ROOT = Path(__file__).resolve().parent.parent

try:
    __version__ = toml.load(PACKAGE_ROOT / "pyproject.toml")["tool"]["poetry"]["version"]
except FileNotFoundError:  # pragma: no cover
    __version__ = "latest"

USER_AGENT = f"NautilusTrader/{__version__}"


def clean_version_string(version: str) -> str:
    """
    Clean the version string by removing any non-digit leading characters.
    """
    # Check if the version starts with any of the operators and remove them
    specifiers = ["==", ">=", "<=", "^", ">", "<"]
    for s in specifiers:
        version = version.replace(s, "")

    # Only allow digits, dots, a, b, rc characters
    return "".join(c for c in version if c.isdigit() or c in ".abrc")


def get_package_version_from_toml(
    package_name: str,
    strip_specifiers: bool = False,
) -> str:
    """
    Return the package version specified in the given `toml_file` for the given
    `package_name`.
    """
    with resources.path("your_package_name", "pyproject.toml") as toml_path:
        data = toml.load(toml_path)
        version = data["tool"]["poetry"]["dependencies"][package_name]["version"]
        if strip_specifiers:
            version = clean_version_string(version)
        return version
