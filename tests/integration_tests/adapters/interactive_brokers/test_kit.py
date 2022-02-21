import pathlib
import pickle

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.instruments.equity import Equity
from tests import TESTS_PACKAGE_ROOT


TEST_PATH = pathlib.Path(TESTS_PACKAGE_ROOT + "/integration_tests/adapters/interactive_brokers/")
RESPONSES_PATH = pathlib.Path(TEST_PATH / "responses")
STREAMING_PATH = pathlib.Path(TEST_PATH / "streaming")
CONTRACT_PATH = pathlib.Path(RESPONSES_PATH / "contracts")


class IBTestStubs:
    @staticmethod
    def contract_details(symbol: str):
        return pickle.load(  # noqa: S301
            open(RESPONSES_PATH / f"contracts/{symbol.upper()}.pkl", "rb")
        )

    @staticmethod
    def instrument(symbol: str, venue: str) -> Equity:
        instrument_id = InstrumentId.from_str(f"{symbol}.{venue}")
        contract_details = IBTestStubs.contract_details(symbol)
        return parse_instrument(instrument_id=instrument_id, contract_details=contract_details)

    @staticmethod
    def market_depth(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_depth.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_ticker.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301
