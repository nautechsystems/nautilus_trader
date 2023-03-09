# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import Venue


@pytest.fixture()
def venue() -> Venue:
    raise NotImplementedError("`venue` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def data_client():
    raise NotImplementedError("`data_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def exec_client():
    raise NotImplementedError("`exec_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def instrument():
    raise NotImplementedError("`instrument` needs to be implemented in adapter `conftest.py`")


@pytest.fixture()
def account_state() -> AccountState:
    raise NotImplementedError("`account_state` needs to be implemented in adapter `conftest.py`")
