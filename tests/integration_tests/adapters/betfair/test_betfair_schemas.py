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
import msgspec

from nautilus_trader.adapters.betfair.client.schema.streaming import MCM
from nautilus_trader.adapters.betfair.client.schema.streaming import BestAvailableToBack
from nautilus_trader.adapters.betfair.client.schema.streaming import BestAvailableToLay
from tests.integration_tests.adapters.betfair.test_kit import BetfairStreaming


class TestBetfairSchemas:
    def test_market_update(self):
        raw = BetfairStreaming.load("streaming_mcm_UPDATE.json", kind=None)
        message: MCM = msgspec.json.decode(raw, type=MCM)
        assert isinstance(message, MCM)
        assert message.pt == 1471370160471
        market_change = message.mc[0]
        assert market_change.id == "1.180727728"
        runner = market_change.rc[0]
        assert runner.id == 3316816
        assert runner.batb == [BestAvailableToBack(level=0, price=4.7, volume=4.33)]
        assert runner.batl == [BestAvailableToLay(level=0, price=4.7, volume=0)]
