# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
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

from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import PropertyMock

import pandas as pd
import pytest

from nautilus_trader.adapters.databento.config import DatabentoDataClientConfig
from nautilus_trader.adapters.databento.data import DatabentoDataClient
from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.adapters.databento.providers import DatabentoInstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.test_kit.providers import TestInstrumentProvider


@pytest.fixture
def venue():
    return Venue("GLBX")


@pytest.fixture
def instrument():
    return TestInstrumentProvider.es_future(2021, 12)


@pytest.fixture
def instrument_provider():
    mock = MagicMock(spec=DatabentoInstrumentProvider)
    mock.initialize = AsyncMock()
    mock.get_all = MagicMock(return_value={})
    return mock


@pytest.fixture
def data_client():
    pass  # Not applicable (use databento_client fixture)


@pytest.fixture
def exec_client():
    pass  # Not applicable


@pytest.fixture
def account_state():
    pass  # Not applicable


@pytest.fixture
def mock_http_client():
    mock = MagicMock(spec=nautilus_pyo3.DatabentoHistoricalClient)
    type(mock).key = PropertyMock(return_value="test-api-key")
    mock.get_range_quotes = AsyncMock(return_value=[])
    mock.get_range_trades = AsyncMock(return_value=[])
    mock.get_range_bars = AsyncMock(return_value=[])
    mock.get_order_book_depth10 = AsyncMock(return_value=[])
    return mock


@pytest.fixture
def mock_loader():
    mock = MagicMock(spec=DatabentoDataLoader)
    mock.get_dataset_for_venue = MagicMock(return_value="GLBX.MDP3")
    return mock


@pytest.fixture
def databento_client(
    event_loop,
    mock_http_client,
    msgbus,
    cache,
    live_clock,
    instrument_provider,
    mock_loader,
):
    config = DatabentoDataClientConfig(api_key="test-api-key")

    client = DatabentoDataClient(
        loop=event_loop,
        http_client=mock_http_client,
        msgbus=msgbus,
        cache=cache,
        clock=live_clock,
        instrument_provider=instrument_provider,
        loader=mock_loader,
        config=config,
    )

    # Mock time range resolution to avoid dataset range HTTP calls
    client._resolve_time_range_for_request = AsyncMock(
        return_value=(
            pd.Timestamp("2024-01-01", tz="UTC"),
            pd.Timestamp("2024-01-02", tz="UTC"),
        ),
    )

    return client


@pytest.fixture
def data_responses(msgbus, data_engine):
    responses = []
    msgbus.deregister(endpoint="DataEngine.response", handler=data_engine.response)
    msgbus.register(endpoint="DataEngine.response", handler=responses.append)
    return responses
