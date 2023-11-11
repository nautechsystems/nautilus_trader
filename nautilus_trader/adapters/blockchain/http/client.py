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
from web3 import Web3

from nautilus_trader.common.clock import LiveClock
from nautilus_trader.common.logging import Logger


class BlockchainHttpClient:
    def __init__(
        self,
        clock: LiveClock,
        logger: Logger,
        rpc_url: str,
    ):
        self.clock: LiveClock = clock
        self.logger: Logger = logger
        self.w3 = Web3(Web3.HTTPProvider(rpc_url))

    def get_latest_block(self):
        return self.w3.eth.get_block("latest")
