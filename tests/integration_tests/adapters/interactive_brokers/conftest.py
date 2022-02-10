import pickle

import pytest

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = TESTS_PACKAGE_ROOT + "/integration_tests/adapters/interactive_brokers/responses/"


@pytest.fixture()
def contract_details_aapl():
    return pickle.load(open(TEST_PATH + "contracts/AAPL.pkl", "rb"))  # noqa S301


@pytest.fixture()
def instrument_aapl(contract_details_aapl):
    instrument_id = InstrumentId(symbol=Symbol("AAPL"), venue=Venue("NASDAQ"))
    return parse_instrument(instrument_id=instrument_id, contract_details=contract_details_aapl)
