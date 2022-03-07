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
import os

import pytest

from nautilus_trader.adapters.betfair.client.core import BetfairClient


class TestBetfairDefinitions:
    def setup(self):
        self.loop = asyncio.get_event_loop()
        self.loop.set_debug(True)
        self.client: BetfairClient = BetfairClient(
            username=os.environ["BETFAIR_USERNAME"],
            password=os.environ["BETFAIR_PASSWORD"],
            app_key=os.environ["BETFAIR_APP_KEY"],
            cert_dir=os.environ["BETFAIR_CERTS"],
        )
        self.data_

    @pytest.mark.asyncio
    async def test_conn(self):
        await self.client.connect()
