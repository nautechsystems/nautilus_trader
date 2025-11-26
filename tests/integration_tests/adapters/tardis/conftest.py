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

from pathlib import Path

import pytest

from nautilus_trader import PACKAGE_ROOT


def get_test_data_path(file_name: str) -> Path:
    """
    Get path to test data file in the tardis test data directory.
    """
    path = (
        PACKAGE_ROOT
        / "crates"
        / "adapters"
        / "tardis"
        / "src"
        / "tests"
        / "data"
        / "csv"
        / file_name
    )
    assert path.exists(), f"Test data file not found: {path}"
    return path


@pytest.fixture
def instrument_provider():
    pass  # Not applicable


@pytest.fixture
def data_client():
    pass  # Not applicable


@pytest.fixture
def exec_client():
    pass  # Not applicable


@pytest.fixture
def instrument():
    pass  # Not applicable


@pytest.fixture
def account_state():
    pass  # Not applicable
