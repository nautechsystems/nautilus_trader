# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2022 Nautech Systems Pty Ltd. All rights reserved.
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

import asyncio
import copy
import os
import pickle

from ib_insync import Contract
from ib_insync import ContractDetails
from ib_insync import Forex
from ib_insync import Future
from ib_insync import Option
from ib_insync import Stock

from nautilus_trader.adapters.interactive_brokers.factories import get_cached_ib_client
from tests.integration_tests.adapters.interactive_brokers.test_kit import CONTRACT_PATH
from tests.integration_tests.adapters.interactive_brokers.test_kit import STREAMING_PATH


CONTRACTS = [
    {"cls": Stock, "symbol": "AAPL", "exchange": "SMART", "currency": "USD"},
    {"cls": Future, "symbol": "CL", "exchange": "NYMEX", "currency": "USD"},
    {
        "cls": Option,
        "symbol": "AAPL",
        "exchange": "SMART",
        "currency": "USD",
        "strike": 160.0,
    },
    {"cls": Forex, "symbol": "AUD", "exchange": "IDEALPRO"},
]


def generate_test_data():
    ib = get_cached_ib_client(os.environ["TWS_USERNAME"], os.environ["TWS_PASSWORD"])
    for spec in CONTRACTS:
        cls = spec.pop("cls")
        results = ib.reqContractDetails(cls(**spec))
        print(f"Found {len(results)}, using first instance")
        c: ContractDetails = results[0]
        with open(f"./responses/contracts/{c.contract.localSymbol}.pkl", "wb") as f:
            f.write(pickle.dumps(c))


def generate_contract(sec_type, filename: str, **kwargs):
    ib = get_cached_ib_client(os.environ["TWS_USERNAME"], os.environ["TWS_PASSWORD"])
    [contract] = ib.qualifyContracts(Contract.create(secType=sec_type, **kwargs))
    [details] = ib.reqContractDetails(contract=contract)

    with open(CONTRACT_PATH / f"{filename}.pkl".lower(), "wb") as f:
        f.write(pickle.dumps(details))


async def generate_market_depth(n_records=50):
    ib = get_cached_ib_client(os.environ["TWS_USERNAME"], os.environ["TWS_PASSWORD"])
    [contract] = ib.qualifyContracts(Forex("EURUSD"))
    ticker = ib.reqMktDepth(contract=contract)

    data = []

    def record(x):
        data.append(copy.copy(x))

    ticker.updateEvent += record

    while len(data) < n_records:
        await asyncio.sleep(0.1)

    with open(STREAMING_PATH / "eurusd_depth.pkl", "wb") as f:
        f.write(pickle.dumps(data))


async def generate_ticks(n_records=50):
    ib = get_cached_ib_client(os.environ["TWS_USERNAME"], os.environ["TWS_PASSWORD"])
    [contract] = ib.qualifyContracts(Forex("EURUSD"))
    ticker = ib.reqMktData(contract=contract)

    data = []

    def record(x):
        data.append(copy.copy(x))

    ticker.updateEvent += record

    while len(data) < n_records:
        await asyncio.sleep(0.1)

    with open(STREAMING_PATH / "eurusd_ticker.pkl", "wb") as f:
        f.write(pickle.dumps(data))


if __name__ == "__main__":
    pass
    # generate_test_data()
    # asyncio.run(generate_market_depth())
    generate_contract(sec_type="CASH", filename="eurusd", pair="EURUSD")
