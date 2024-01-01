import functools
from unittest.mock import MagicMock
from unittest.mock import Mock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.interactive_brokers.client.common import Subscription
from nautilus_trader.adapters.interactive_brokers.parsing.data import what_to_show
from nautilus_trader.model.data import BarType
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestDataStubs
from tests.integration_tests.adapters.interactive_brokers.test_kit import IBTestProviderStubs


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
async def test_subscribe_ticks(ib_client):
    # Arrange
    ib_client._request_id_seq = 999
    instrument_id = IBTestProviderStubs.aapl_instrument().id
    contract = IBTestDataStubs.contract()
    tick_type = "BidAsk"
    ib_client._eclient.reqTickByTickData = Mock()

    # Act
    await ib_client.subscribe_ticks(instrument_id, contract, tick_type)

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
    instrument_id = IBTestProviderStubs.aapl_instrument().id
    contract = IBTestDataStubs.contract()
    tick_type = "BidAsk"
    ib_client._eclient.reqTickByTickData = Mock()
    ib_client._eclient.cancelTickByTickData = Mock()
    await ib_client.subscribe_ticks(instrument_id, contract, tick_type)

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
    contract = IBTestDataStubs.contract()
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
    contract = IBTestDataStubs.contract()
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
    contract = IBTestDataStubs.contract()
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
    contract = IBTestDataStubs.contract()
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
    contract = IBTestDataStubs.contract()
    use_rth = True
    end_date_time = "20240101-010000"
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
        endDateTime=end_date_time,
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
    contract = IBTestDataStubs.contract()
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


def test_process_bar_data(ib_client):
    # Arrange

    # Act

    # Assert
    pass


def test_process_trade_ticks(ib_client):
    # Arrange

    # Act

    # Assert
    pass
