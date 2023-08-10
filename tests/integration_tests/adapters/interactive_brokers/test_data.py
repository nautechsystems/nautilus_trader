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

import asyncio
import datetime

import pytest
from ib_insync import Contract
from ib_insync import Ticker

from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import BookType
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


pytestmark = pytest.mark.skip(reason="Skip due currently flaky mocks")


def instrument_setup(data_client, instrument, contract_details):
    data_client.instrument_provider.contract_details[instrument.id.value] = contract_details
    data_client.instrument_provider.contract_id_to_instrument_id[
        contract_details.contract.conId
    ] = instrument.id
    data_client.instrument_provider.add(instrument)


@pytest.mark.asyncio()
async def test_connect(data_client):
    data_client.connect()
    await asyncio.sleep(0)
    await asyncio.sleep(0)
    assert data_client.is_connected


@pytest.mark.asyncio()
async def test_subscribe_trade_ticks(data_client):
    # Arrange
    instrument_aapl = IBTestProviderStubs.aapl_instrument()
    data_client.instrument_provider.contract_details[
        instrument_aapl.id.value
    ] = IBTestProviderStubs.aapl_equity_contract_details()

    # Act
    data_client.subscribe_trade_ticks(instrument_id=instrument_aapl.id)
    await asyncio.sleep(0)

    # Assert
    kwargs = data_client._client.reqMktData.call_args.kwargs
    expected = {
        "contract": Contract(
            secType="STK",
            conId=265598,
            symbol="AAPL",
            exchange="SMART",
            primaryExchange="NASDAQ",
            currency="USD",
            localSymbol="AAPL",
            tradingClass="NMS",
        ),
    }
    assert kwargs == expected


@pytest.mark.asyncio()
async def test_subscribe_order_book_deltas(data_client, instrument):
    # Arrange
    instrument = IBTestProviderStubs.aapl_instrument()
    instrument_setup(data_client, instrument, IBTestProviderStubs.aapl_equity_contract_details())

    # Act
    data_client.subscribe_order_book_snapshots(
        instrument_id=instrument.id,
        book_type=BookType.L2_MBP,
    )
    await asyncio.sleep(0)

    # Assert
    kwargs = data_client._client.reqMktDepth.call_args.kwargs
    expected = {
        "contract": Contract(
            secType="STK",
            conId=265598,
            symbol="AAPL",
            exchange="SMART",
            primaryExchange="NASDAQ",
            currency="USD",
            localSymbol="AAPL",
            tradingClass="NMS",
        ),
        "numRows": 5,
    }
    assert kwargs == expected


@pytest.mark.asyncio()
async def test_on_book_update(data_client):
    # Arrange
    instrument_setup(
        data_client,
        IBTestProviderStubs.eurusd_instrument(),
        IBTestProviderStubs.eurusd_forex_contract_details(),
    )

    # Act
    for ticker in IBTestDataStubs.market_depth(name="eurusd"):
        data_client._on_order_book_snapshot(ticker=ticker, book_type=BookType.L2_MBP)


@pytest.mark.asyncio()
async def test_on_ticker_update(data_client):
    # Arrange
    instrument_setup(
        data_client,
        IBTestProviderStubs.eurusd_instrument(),
        IBTestProviderStubs.eurusd_forex_contract_details(),
    )

    # Act
    for ticker in IBTestDataStubs.tickers("eurusd"):
        data_client._on_trade_ticker_update(ticker=ticker)


@pytest.mark.asyncio()
async def test_on_quote_tick_update(data_client):
    # Arrange
    instrument_setup(
        data_client,
        IBTestProviderStubs.eurusd_instrument(),
        IBTestProviderStubs.eurusd_forex_contract_details(),
    )
    contract = IBTestProviderStubs.eurusd_forex_contract_details().contract
    ticker = Ticker(
        time=datetime.datetime(2022, 3, 4, 6, 8, 36, 992576, tzinfo=datetime.timezone.utc),
        bid=99.45,
        ask=99.5,
        bidSize=44600.0,
        askSize=29500.0,
    )

    # Act
    data_client._on_quote_tick_update(tick=ticker, contract=contract)


@pytest.mark.asyncio()
async def test_on_quote_tick_update_nans(data_client, instrument):
    # Arrange
    instrument_setup(data_client, instrument, IBTestProviderStubs.aapl_equity_contract_details())
    contract = IBTestProviderStubs.aapl_equity_contract_details().contract
    ticker = Ticker(
        time=datetime.datetime(2022, 3, 4, 6, 8, 36, 992576, tzinfo=datetime.timezone.utc),
        bidSize=44600.0,
        askSize=29500.0,
    )
    data = []
    data_client._handle_data = data.append

    # Act
    data_client._on_quote_tick_update(tick=ticker, contract=contract)
    update = data[0]
    await asyncio.sleep(0)

    # Assert
    expected = QuoteTick.from_dict(
        {
            "type": "QuoteTick",
            "instrument_id": "AAPL.NASDAQ",
            "bid_price": "0.00",
            "ask_price": "0.00",
            "bid_size": "44600",
            "ask_size": "29500",
            "ts_event": 0,
            "ts_init": 0,
        },
    )
    assert update == expected
