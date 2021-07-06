# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.betfair.sockets import BetfairMarketStreamClient
from nautilus_trader.adapters.betfair.sockets import BetfairOrderStreamClient


def test_unique_id(betfair_client, live_logger):
    clients = [
        BetfairMarketStreamClient(client=betfair_client, logger=live_logger, message_handler=len),
        BetfairOrderStreamClient(client=betfair_client, logger=live_logger, message_handler=len),
        BetfairMarketStreamClient(client=betfair_client, logger=live_logger, message_handler=len),
    ]
    result = [c.unique_id for c in clients]
    assert result == sorted(set(result))
