# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import copy
import functools
from decimal import Decimal
from unittest.mock import AsyncMock
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pandas as pd
import pytest
from ibapi.common import BarData
from ibapi.common import HistoricalTickLast
from ibapi.common import TickAttribBidAsk
from ibapi.common import TickAttribLast

from nautilus_trader.adapters.interactive_brokers.client.common import Request
from nautilus_trader.adapters.interactive_brokers.client.common import Subscription
from nautilus_trader.adapters.interactive_brokers.parsing.data import what_to_show
from nautilus_trader.model.data import Bar
from nautilus_trader.model.data import BarType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestContractStubs


@pytest.mark.asyncio
async def test_subscribe(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    subscription_method = MagicMock()
    cancellation_method = MagicMock()
    name = "test_subscription"
    args = (1, 2, 3)
    kwargs = {"a": 1, "b": 2}

    # Act
    result = await ib_client._subscribe(
        name,
        subscription_method,
        cancellation_method,
        *args,
        **kwargs,
    )

    # Assert
    subscription_method.assert_called_once_with(
        999,
        1,
        2,
        3,
        a=1,
        b=2,
    )
    subscription = Subscription(
        req_id=999,
        name="test_subscription",
        handle=functools.partial(subscription_method, 10000, 1, 2, 3, a=1, b=2),
        cancel=functools.partial(cancellation_method, 10000),
        last=None,
    )
    assert hash(subscription) == hash(result)
    cancellation_method.assert_not_called()


@pytest.mark.asyncio
async def test_subscribe_historical_subscription_not_called(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    ib_client.subscribe_historical_bars = MagicMock()
    subscription_method = ib_client.subscribe_historical_bars
    cancellation_method = MagicMock()
    name = "test_subscription"
    args = (1, 2, 3)
    kwargs = {"a": 1, "b": 2}

    # Act
    subscription = await ib_client._subscribe(
        name,
        subscription_method,
        cancellation_method,
        *args,
        **kwargs,
    )

    # Assert
    subscription_method.assert_not_called()
    assert isinstance(subscription, Subscription)


@pytest.mark.asyncio
async def test_subscribe_subscription_always_returned(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    ib_client.subscribe_historical_bars = MagicMock()
    subscription_method = ib_client.subscribe_historical_bars
    cancellation_method = MagicMock()
    name = "test_subscription"
    args = (1, 2, 3)
    kwargs = {"a": 1, "b": 2}
    await ib_client._subscribe(
        name,
        subscription_method,
        cancellation_method,
        *args,
        **kwargs,
    )

    # Act
    subscription = await ib_client._subscribe(
        name,
        subscription_method,
        cancellation_method,
        *args,
        **kwargs,
    )

    # Assert
    assert isinstance(subscription, Subscription)


@pytest.mark.asyncio
async def test_subscribe_ticks(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    instrument_id = IBTestContractStubs.aapl_instrument().id
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    tick_type = "BidAsk"
    ignore_size = True
    ib_client._eclient.reqTickByTickData = Mock()

    # Act
    await ib_client.subscribe_ticks(instrument_id, contract, tick_type, ignore_size)

    # Assert
    ib_client._eclient.reqTickByTickData.assert_called_once_with(
        999,
        contract,
        tick_type,
        0,
        True,
    )


@pytest.mark.asyncio
async def test_unsubscribe_ticks(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    instrument_id = IBTestContractStubs.aapl_instrument().id
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    tick_type = "BidAsk"
    ignore_size = True
    ib_client._eclient.reqTickByTickData = Mock()
    ib_client._eclient.cancelTickByTickData = Mock()
    await ib_client.subscribe_ticks(instrument_id, contract, tick_type, ignore_size)

    # Act
    await ib_client.unsubscribe_ticks(instrument_id, tick_type)

    # Assert
    ib_client._eclient.cancelTickByTickData.assert_called_once_with(
        reqId=999,
    )


@pytest.mark.asyncio
async def test_subscribe_realtime_bars(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    bar_type = BarType.from_str("AAPL.SMART-5-SECOND-BID-EXTERNAL")
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    use_rth = True
    ib_client._eclient.reqRealTimeBars = Mock()

    # Act
    await ib_client.subscribe_realtime_bars(bar_type, contract, use_rth)

    # Assert
    ib_client._eclient.reqRealTimeBars.assert_called_once_with(
        999,
        contract,
        bar_type.spec.step,
        what_to_show(bar_type),
        use_rth,
        [],
    )


@pytest.mark.asyncio
async def test_unsubscribe_realtime_bars(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    bar_type = BarType.from_str("AAPL.SMART-5-SECOND-BID-EXTERNAL")
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    use_rth = True
    ib_client._eclient.reqRealTimeBars = Mock()
    ib_client._eclient.cancelRealTimeBars = Mock()
    await ib_client.subscribe_realtime_bars(bar_type, contract, use_rth)

    # Act
    await ib_client.unsubscribe_realtime_bars(bar_type)

    # Assert
    ib_client._eclient.cancelRealTimeBars.assert_called_once_with(
        reqId=999,
    )


@pytest.mark.asyncio
async def test_subscribe_historical_bars(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    bar_type = BarType.from_str("AAPL.SMART-5-SECOND-BID-EXTERNAL")
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    use_rth = True
    handle_revised_bars = True
    ib_client._eclient.reqHistoricalData = Mock()

    # Act
    await ib_client.subscribe_historical_bars(
        bar_type,
        contract,
        use_rth,
        handle_revised_bars,
    )

    # Assert
    ib_client._eclient.reqHistoricalData.assert_called_once_with(
        reqId=999,
        contract=contract,
        endDateTime="",
        durationStr="1500 S",
        barSizeSetting="5 secs",
        whatToShow="BID",
        useRTH=use_rth,
        formatDate=2,
        keepUpToDate=True,
        chartOptions=[],
    )


@pytest.mark.asyncio
async def test_unsubscribe_historical_bars(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    bar_type = BarType.from_str("AAPL.SMART-5-SECOND-BID-EXTERNAL")
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    use_rth = True
    handle_revised_bars = True
    ib_client._eclient.reqHistoricalData = Mock()
    ib_client._eclient.cancelHistoricalData = Mock()
    await ib_client.subscribe_historical_bars(
        bar_type,
        contract,
        use_rth,
        handle_revised_bars,
    )

    # Act
    await ib_client.unsubscribe_historical_bars(bar_type)

    # Assert
    ib_client._eclient.cancelHistoricalData.assert_called_once_with(
        reqId=999,
    )


@pytest.mark.asyncio
async def test_get_historical_bars(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    bar_type = BarType.from_str("AAPL.SMART-5-SECOND-BID-EXTERNAL")
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    use_rth = True
    end_date_time = pd.Timestamp("20240101-010000+0000")
    duration = "5 S"
    ib_client._eclient.reqHistoricalData = Mock()

    # Act
    with patch("asyncio.wait_for"):
        await ib_client.get_historical_bars(
            bar_type,
            contract,
            use_rth,
            end_date_time,
            duration,
        )

    # Assert
    ib_client._eclient.reqHistoricalData.assert_called_once_with(
        reqId=999,
        contract=contract,
        endDateTime=end_date_time.strftime("%Y%m%d %H:%M:%S %Z"),
        durationStr=duration,
        barSizeSetting="5 secs",
        whatToShow="BID",
        useRTH=use_rth,
        formatDate=2,
        keepUpToDate=False,
        chartOptions=[],
    )


@pytest.mark.asyncio
async def test_get_historical_ticks(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    tick_type = "BidAsk"
    start_date_time = "20240101 01:00:00"
    end_date_time = "20240101 02:00:00"
    use_rth = True
    ib_client._eclient.reqHistoricalTicks = Mock()

    # Act
    with patch("asyncio.wait_for"):
        await ib_client.get_historical_ticks(
            contract,
            tick_type,
            start_date_time,
            end_date_time,
            use_rth,
        )

    # Assert
    ib_client._eclient.reqHistoricalTicks.assert_called_once_with(
        reqId=999,
        contract=contract,
        startDateTime=start_date_time,
        endDateTime=end_date_time,
        numberOfTicks=1000,
        whatToShow="BidAsk",
        useRth=use_rth,
        ignoreSize=False,
        miscOptions=[],
    )


@pytest.mark.asyncio
async def test_ib_bar_to_nautilus_bar(ib_client):
    # Arrange
    bar_type_str = "AAPL.NASDAQ-5-SECOND-BID-INTERNAL"
    bar_type = BarType.from_str(bar_type_str)
    bar = BarData()
    bar.date = "1704067200"
    bar.open = 100.01
    bar.high = 101.00
    bar.low = 99.01
    bar.close = 100.50
    bar.volume = Decimal(100)
    bar.wap = Decimal(-1)
    bar.barCount = -1
    ts_init = 1704067205000000000
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    # Act
    result = await ib_client._ib_bar_to_nautilus_bar(bar_type, bar, ts_init, is_revision=False)

    # Assert
    assert result.bar_type == BarType.from_str(bar_type_str)
    assert result.open == Price(100.01, precision=2)
    assert result.high == Price(101.00, precision=2)
    assert result.low == Price(99.01, precision=2)
    assert result.close == Price(100.50, precision=2)
    assert result.volume == Quantity(100, precision=0)
    assert result.ts_event == 1704067200000000000
    assert result.ts_init == 1704067205000000000
    assert result.is_revision is False


@pytest.mark.asyncio
async def test_process_bar_data(ib_client):
    # Arrange
    bar_type_str = "AAPL.NASDAQ-5-SECOND-BID-INTERNAL"
    previous_bar = BarData()
    previous_bar.date = "1704067200"
    previous_bar.open = 100.01
    previous_bar.high = 101.00
    previous_bar.low = 99.01
    previous_bar.close = 100.50
    previous_bar.volume = Decimal(100)
    previous_bar.wap = Decimal(-1)
    previous_bar.barCount = -1
    ib_client._bar_type_to_last_bar[bar_type_str] = previous_bar
    ib_client._clock.set_time(1704067205000000000)
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())
    bar = copy.deepcopy(previous_bar)
    bar.date = "1704067205"

    # Act
    result = await ib_client._process_bar_data(
        bar_type_str,
        bar,
        handle_revised_bars=False,
        historical=False,
    )

    # Assert
    assert isinstance(result, Bar)
    assert result.bar_type == BarType.from_str(bar_type_str)
    assert result.open == Price(100.01, precision=2)
    assert result.high == Price(101.00, precision=2)
    assert result.low == Price(99.01, precision=2)
    assert result.close == Price(100.50, precision=2)
    assert result.volume == Quantity(100, precision=0)
    assert result.ts_event == 1704067200000000000
    assert result.ts_init == 1704067205000000000
    assert result.is_revision is False


# @pytest.mark.skip(reason="WIP")
@pytest.mark.asyncio
async def test_process_trade_ticks(ib_client):
    # Arrange
    mock_request = Mock(spec=Request)
    mock_request.name = ["AAPL.NASDAQ"]
    mock_request.result = []
    ib_client._requests = Mock()
    ib_client._requests.get.return_value = mock_request

    request_id = 1
    trade_tick_1 = HistoricalTickLast()
    trade_tick_1.time = 1704067200
    trade_tick_1.price = 100.01
    trade_tick_1.size = 100
    trade_tick_2 = HistoricalTickLast()
    trade_tick_2.time = 1704067205
    trade_tick_2.price = 105.01
    trade_tick_2.size = 200
    ticks = [trade_tick_1, trade_tick_2]

    # Act
    await ib_client._process_trade_ticks(request_id, ticks)

    # Assert
    assert len(mock_request.result) == 2

    result_1 = mock_request.result[0]
    assert result_1.instrument_id == InstrumentId.from_str("AAPL.NASDAQ")
    assert result_1.price == Price(100.01, precision=2)
    assert result_1.size == Quantity(100, precision=0)
    assert result_1.aggressor_side == AggressorSide.NO_AGGRESSOR
    assert result_1.trade_id == TradeId("1704067200-100.01-100")
    assert result_1.ts_event == 1704067200000000000
    assert result_1.ts_init == 1704067200000000000

    result_2 = mock_request.result[1]
    assert result_2.instrument_id == InstrumentId.from_str("AAPL.NASDAQ")
    assert result_2.price == Price(105.01, precision=2)
    assert result_2.size == Quantity(200, precision=0)
    assert result_2.aggressor_side == AggressorSide.NO_AGGRESSOR
    assert result_2.trade_id == TradeId("1704067205-105.01-200")
    assert result_2.ts_event == 1704067205000000000
    assert result_2.ts_init == 1704067205000000000


@pytest.mark.asyncio
async def test_tickByTickBidAsk(ib_client):
    # Arrange
    ib_client._clock.set_time(1704067205000000000)
    mock_subscription = Mock(spec=Subscription)
    mock_subscription.name = ["AAPL.NASDAQ"]
    ib_client._subscriptions = Mock()
    ib_client._subscriptions.get.return_value = mock_subscription
    ib_client._handle_data = AsyncMock()

    # Act
    await ib_client.process_tick_by_tick_bid_ask(
        req_id=1,
        time=1704067200,
        bid_price=100.01,
        ask_price=100.02,
        bid_size=Decimal(100),
        ask_size=Decimal(200),
        tick_attrib_bid_ask=TickAttribBidAsk(),
    )

    # Assert
    quote_tick = QuoteTick(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        bid_price=Price(100.01, precision=2),
        ask_price=Price(100.02, precision=2),
        bid_size=Quantity(100, precision=0),
        ask_size=Quantity(200, precision=0),
        ts_event=1704067200000000000,
        ts_init=1704067205000000000,
    )
    ib_client._handle_data.assert_called_once_with(quote_tick)


@pytest.mark.asyncio
async def test_tickByTickAllLast(ib_client):
    # Arrange
    ib_client._clock.set_time(1704067205000000000)
    mock_subscription = Mock(spec=Subscription)
    mock_subscription.name = ["AAPL.NASDAQ"]
    ib_client._subscriptions = Mock()
    ib_client._subscriptions.get.return_value = mock_subscription
    ib_client._handle_data = AsyncMock()

    # Act
    await ib_client.process_tick_by_tick_all_last(
        req_id=1,
        tick_type="Last",
        time=1704067200,
        price=100.01,
        size=Decimal(100),
        tick_attrib_last=TickAttribLast(),
        exchange="",
        special_conditions="",
    )

    # Assert
    trade_tick = TradeTick(
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        price=Price(100.01, precision=2),
        size=Quantity(100, precision=0),
        aggressor_side=AggressorSide.NO_AGGRESSOR,
        trade_id=TradeId("1704067200-100.01-100"),
        ts_event=1704067200000000000,
        ts_init=1704067205000000000,
    )
    ib_client._handle_data.assert_called_once_with(trade_tick)


@pytest.mark.asyncio
async def test_realtimeBar(ib_client):
    # Arrange
    ib_client._clock.set_time(1704067205000000000)
    mock_subscription = Mock(spec=Subscription)
    bar_type_str = "AAPL.NASDAQ-5-SECOND-BID-INTERNAL"
    mock_subscription.name = bar_type_str
    ib_client._subscriptions = Mock()
    ib_client._subscriptions.get.return_value = mock_subscription
    ib_client._handle_data = AsyncMock()

    # Act
    await ib_client.process_realtime_bar(
        req_id=1,
        time=1704067200,
        open_=100.01,
        high=101.00,
        low=99.01,
        close=100.50,
        volume=Decimal(100),
        wap=Decimal(-1),
        count=Decimal(-1),
    )

    # Assert
    bar = Bar(
        bar_type=BarType.from_str(bar_type_str),
        open=Price(100.01, precision=2),
        high=Price(101.00, precision=2),
        low=Price(99.01, precision=2),
        close=Price(100.50, precision=2),
        volume=Quantity(100, precision=0),
        ts_event=1704067200000000000,
        ts_init=1704067205000000000,
        is_revision=False,
    )
    ib_client._handle_data.assert_called_once_with(bar)


@pytest.mark.asyncio
async def test_get_price_retrieval(ib_client):
    """
    Test case for retrieving price data.
    """
    # Arrange
    # Set up the request ID and mock the necessary methods
    ib_client._request_id_seq = 999
    contract = IBTestContractStubs.aapl_equity_ib_contract()
    tick_type = "MidPoint"
    ib_client._eclient.reqMktData = MagicMock()

    # Act
    # Call the method to get the price
    await ib_client.get_price(contract, tick_type)

    # Assert
    # Verify that the market data request was made with the correct parameters
    ib_client._eclient.reqMktData.assert_called_once_with(
        999,
        contract,
        tick_type,
        False,
        False,
        [],
    )
