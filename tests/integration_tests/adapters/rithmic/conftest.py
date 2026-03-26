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

import pytest

from nautilus_trader.adapters.rithmic import RITHMIC_VENUE
from nautilus_trader.adapters.rithmic.config import RithmicDataClientConfig
from nautilus_trader.adapters.rithmic.providers import RithmicInstrumentProvider
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


@pytest.fixture
def venue() -> Venue:
    return RITHMIC_VENUE


@pytest.fixture
def instrument():
    return TestInstrumentProvider.future(
        symbol="MNQM6",
        underlying="MNQ",
        venue=RITHMIC_VENUE.value,
        exchange="XCME",
    )


@pytest.fixture
def account_state(account_id) -> AccountState:
    return AccountState(
        account_id=account_id,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        reported=True,
        balances=[
            AccountBalance(
                total=Money(100_000, USD),
                locked=Money(0, USD),
                free=Money(100_000, USD),
            ),
        ],
        margins=[],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def instrument_provider() -> RithmicInstrumentProvider:
    return RithmicInstrumentProvider(
        RithmicDataClientConfig(
            username="u",
            password="p",
            system_name="Apex",
        ),
    )


@pytest.fixture
def data_client():
    return None


@pytest.fixture
def exec_client():
    return None
