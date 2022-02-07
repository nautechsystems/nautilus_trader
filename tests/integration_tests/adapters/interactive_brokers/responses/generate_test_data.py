import pickle
from functools import lru_cache

from ib_insync import IB
from ib_insync import ContractDetails
from ib_insync import Forex
from ib_insync import Future
from ib_insync import Option
from ib_insync import Stock

from nautilus_trader.adapters.ib.gateway import IBGateway


CONTRACTS = [
    {"cls": Stock, "symbol": "AAPL", "exchange": "SMART", "currency": "USD"},
    {"cls": Future, "symbol": "CL", "exchange": "NYMEX", "currency": "USD"},
    {
        "cls": Option,
        "symbol": "AAPL",
        "exchange": "SMART",
        "currency": "USD",
        "strike": 160.0,
        "lastTradeDateOrContractMonth": "202112",
    },
    {"cls": Forex, "symbol": "AUD", "exchange": "IDEALPRO", "currency": "USD"},
]


@lru_cache()
def get_client() -> IB:
    gw = IBGateway()
    try:
        gw.start()
    except ValueError:
        pass
    return gw.client


def generate_test_data():
    client = get_client()
    for spec in CONTRACTS:
        cls = spec.pop("cls")
        results = client.reqContractDetails(cls(**spec))
        print(f"Found {len(results)}, using first instance")
        c: ContractDetails = results[0]
        with open(f"./responses/contracts/{c.contract.localSymbol}.pkl", "wb") as f:
            f.write(pickle.dumps(c))


if __name__ == "__main__":
    generate_test_data()
