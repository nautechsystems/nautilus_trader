import pytest

from nautilus_trader.adapters.sandbox.config import SandboxExecutionClientConfig
from nautilus_trader.adapters.sandbox.execution import SandboxExecutionClient
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs


@pytest.fixture
def venue() -> Venue:
    return Venue("SANDBOX")


@pytest.fixture
def exec_client(
    instrument,
    event_loop,
    portfolio,
    msgbus,
    cache,
    clock,
    venue,
):
    cache.add_instrument(instrument)  # <-- This might be redundant now

    config = SandboxExecutionClientConfig(
        venue=venue.value,
        starting_balances=["100_000 USD"],
        base_currency="USD",
        account_type="CASH",
    )
    return SandboxExecutionClient(
        loop=event_loop,
        portfolio=portfolio,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        config=config,
    )


@pytest.fixture
def instrument():
    return TestInstrumentProvider.equity("AAPL", "SANDBOX")


@pytest.fixture
def account_state() -> AccountState:
    return TestEventStubs.cash_account_state(account_id=AccountId("SANDBOX-001"))


@pytest.fixture
def data_client():
    pass
