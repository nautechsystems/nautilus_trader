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

import pickle

import ib_insync
from ib_insync import Future


# Requirements:
# - An internet connection
# - Running TWS on local host and port 7497


def contract_details_cl():
    # Write pickled CL contract details to a file

    client = ib_insync.IB()
    client.start()

    contract = Future(
        instrument_id="CL",
        lastTradeDateOrContractMonth="20211119",
        exchange="NYMEX",
        currency="USD",
    )

    details = client.reqContractDetails(contract)

    with open("contract_details_cl.pickle", "wb") as file:
        pickle.dump(details[0], file)


if __name__ == "__main__":
    # Enter function to run
    pass
