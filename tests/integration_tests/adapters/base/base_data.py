# ---------------------------@pytest.mark.usefixtures("components")----------------------------------------------------------------------
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
from typing import Optional

import pytest

from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.base import Instrument
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class BaseDataClient:
    venue: Optional[Venue] = None
    instrument: Optional[Instrument] = None

    @pytest.fixture(autouse=True, scope="function")
    def init(
        self,
        data_client,
        cache,
        instrument,
        venue,
        strategy,
        trader_id,
        strategy_id,
        account_id,
    ):
        self.data_client = data_client
        self.cache = cache
        self.instrument = instrument
        self.venue = venue
        self.strategy = strategy
        self.client_order_id = TestIdStubs.client_order_id()
        self.venue_order_id = TestIdStubs.venue_order_id()
        self.strategy_id = strategy_id
        self.trader_id = trader_id
        self.account_id = account_id
