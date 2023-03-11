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

from nautilus_trader.adapters.sandbox.execution import SandboxExecutionClient
from nautilus_trader.model.events.account import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs


@pytest.fixture()
def venue() -> Venue:
    return Venue("SANDBOX")


@pytest.fixture()
def exec_client(
    instrument,
    event_loop,
    msgbus,
    cache,
    clock,
    logger,
    venue,
):
    SandboxExecutionClient.INSTRUMENTS = [instrument]
    return SandboxExecutionClient(
        loop=event_loop,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        logger=logger,
        venue=venue.value,
        currency="USD",
        balance=100_000,
    )


@pytest.fixture()
def instrument():
    return TestInstrumentProvider.equity("AAPL", "SANDBOX")


@pytest.fixture()
def account_state() -> AccountState:
    return TestEventStubs.cash_account_state(account_id=AccountId("SANDBOX-001"))


@pytest.fixture()
def data_client():
    pass
