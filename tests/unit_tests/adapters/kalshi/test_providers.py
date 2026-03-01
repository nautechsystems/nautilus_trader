# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software distributed under the
#  License is distributed on an "AS IS" BASIS, WITHOUT WARRANTIES OR CONDITIONS OF ANY
#  KIND, either express or implied. See the License for the specific language governing
#  permissions and limitations under the License.
# -------------------------------------------------------------------------------------------------

import pytest
from unittest.mock import AsyncMock

from nautilus_trader.adapters.kalshi.config import KalshiDataClientConfig
from nautilus_trader.adapters.kalshi.providers import KalshiInstrumentProvider


@pytest.mark.asyncio
async def test_provider_load_filters_by_series():
    config = KalshiDataClientConfig(series_tickers=("KXBTC",))
    provider = KalshiInstrumentProvider(config=config)
    # Mock the HTTP client get_markets method
    provider._http_client.get_markets = AsyncMock(return_value=[])
    await provider.load_all_async()
    provider._http_client.get_markets.assert_called_once()
