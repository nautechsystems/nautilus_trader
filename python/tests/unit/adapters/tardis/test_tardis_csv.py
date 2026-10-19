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
Test Tardis CSV conversion behavior.
"""

from pathlib import Path

import pytest

from nautilus_trader.adapters.tardis import convert_tardis_options_chain_csv


def test_convert_options_chain_csv_raises_for_missing_catalog_directory(tmp_path: Path) -> None:
    """
    Raise a Python error instead of panicking when the catalog directory is missing.
    """
    catalog_path = tmp_path / "missing"

    with pytest.raises(ValueError, match="failed to open local storage directory") as exc_info:
        convert_tardis_options_chain_csv([], catalog_path)

    assert str(exc_info.value) == (
        f"failed to open local storage directory '{catalog_path}'; "
        "create it if it does not exist and check access permissions"
    )
    assert not catalog_path.exists()
