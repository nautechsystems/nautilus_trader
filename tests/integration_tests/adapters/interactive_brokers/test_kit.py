import pathlib
import pickle

from ib_insync import Contract

from nautilus_trader.adapters.interactive_brokers.parsing.instruments import parse_instrument
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
    def contract(secType="STK", symbol="AAPL", exchange="NASDAQ", **kwargs):
        return Contract(secType=secType, symbol=symbol, exchange=exchange, **kwargs)

    @staticmethod
    def instrument(symbol: str) -> Equity:
        contract_details = IBTestStubs.contract_details(symbol)
        return parse_instrument(contract_details=contract_details)

    @staticmethod
    def market_depth(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_depth.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301

    @staticmethod
    def tickers(name: str = "eurusd"):
        with open(STREAMING_PATH / f"{name}_ticker.pkl", "rb") as f:
            return pickle.loads(f.read())  # noqa: S301
