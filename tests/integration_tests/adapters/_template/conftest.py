import pytest

from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import Venue


@pytest.fixture
def venue() -> Venue:
    raise NotImplementedError("`venue` needs to be implemented in adapter `conftest.py`")


@pytest.fixture
def data_client():
    raise NotImplementedError("`data_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture
def exec_client():
    raise NotImplementedError("`exec_client` needs to be implemented in adapter `conftest.py`")


@pytest.fixture
def instrument():
    raise NotImplementedError("`instrument` needs to be implemented in adapter `conftest.py`")


@pytest.fixture
def account_state() -> AccountState:
    raise NotImplementedError("`account_state` needs to be implemented in adapter `conftest.py`")
