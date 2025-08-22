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

import asyncio
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
    ib_client._eclient.cancelTickByTickData.assert_called_once_with(999)


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
    ib_client._eclient.cancelRealTimeBars.assert_called_once_with(999)


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
        {},
    )

    # Act
    await ib_client.unsubscribe_historical_bars(bar_type)

    # Assert
    ib_client._eclient.cancelHistoricalData.assert_called_once_with(999)


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
            InstrumentId.from_str("AAPL.NASDAQ"),
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
async def test_process_bar_data_eod_bar_missing_issue(ib_client):
    """
    Test that demonstrates the EOD bar missing issue.

    When handle_revised_bars=False, the last bar of the day (like 11:59 AM) is never
    processed because there's no subsequent bar to trigger its processing.

    """
    # Arrange
    bar_type_str = "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    # Simulate receiving bars throughout the day

    # 11:57 AM bar
    bar_1157 = BarData()
    bar_1157.date = "1704067020"  # 11:57:00 timestamp
    bar_1157.open = 100.00
    bar_1157.high = 100.50
    bar_1157.low = 99.50
    bar_1157.close = 100.25
    bar_1157.volume = Decimal(1000)
    bar_1157.wap = Decimal(-1)
    bar_1157.barCount = -1

    # 11:58 AM bar
    bar_1158 = BarData()
    bar_1158.date = "1704067080"  # 11:58:00 timestamp
    bar_1158.open = 100.25
    bar_1158.high = 100.75
    bar_1158.low = 100.00
    bar_1158.close = 100.50
    bar_1158.volume = Decimal(1200)
    bar_1158.wap = Decimal(-1)
    bar_1158.barCount = -1

    # 11:59 AM bar (EOD bar that should be missing)
    bar_1159 = BarData()
    bar_1159.date = "1704067140"  # 11:59:00 timestamp
    bar_1159.open = 100.50
    bar_1159.high = 101.00
    bar_1159.low = 100.25
    bar_1159.close = 100.75
    bar_1159.volume = Decimal(800)
    bar_1159.wap = Decimal(-1)
    bar_1159.barCount = -1

    # Act - Process bars sequentially as they would arrive

    # Process 11:57 bar (first bar, should return None)
    result_1157 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1157,
        handle_revised_bars=False,
        historical=False,
    )

    # Process 11:58 bar (should return 11:57 bar)
    result_1158 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1158,
        handle_revised_bars=False,
        historical=False,
    )

    # Process 11:59 bar (should return 11:58 bar)
    result_1159 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1159,
        handle_revised_bars=False,
        historical=False,
    )

    # Assert - This demonstrates the issue
    assert result_1157 is None  # First bar returns None (expected)
    assert result_1158 is not None  # Second bar returns first bar (expected)
    assert result_1158.ts_event == 1704067020000000000  # 11:57 bar timestamp

    assert result_1159 is not None  # Third bar returns second bar (expected)
    assert result_1159.ts_event == 1704067080000000000  # 11:58 bar timestamp

    # The problem: The 11:59 bar (EOD bar) is never returned!
    # It's stored in _bar_type_to_last_bar but never processed
    last_stored_bar = ib_client._bar_type_to_last_bar.get(bar_type_str)
    assert last_stored_bar is not None
    assert int(last_stored_bar.date) == 1704067140  # 11:59 bar is stored but not processed

    # In a real scenario, there would be no subsequent bar to trigger processing of the 11:59 bar


@pytest.mark.asyncio
async def test_process_bar_data_completion_timeout_fix(ib_client):
    """
    Test that bars are published after their period completes.

    This test verifies that bars are published when their time period ends, ensuring EOD
    bars and timely delivery without waiting for next bar.

    """
    # Arrange
    bar_type_str = "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    # Mock the _handle_data method to capture processed bars
    processed_bars = []
    original_handle_data = ib_client._handle_data

    async def mock_handle_data(data):
        if isinstance(data, Bar):
            processed_bars.append(data)
        return await original_handle_data(data)

    ib_client._handle_data = mock_handle_data

    # Create test bars
    bar_1157 = BarData()
    bar_1157.date = "1704067020"  # 11:57:00 timestamp
    bar_1157.open = 100.00
    bar_1157.high = 100.50
    bar_1157.low = 99.50
    bar_1157.close = 100.25
    bar_1157.volume = Decimal(1000)
    bar_1157.wap = Decimal(-1)
    bar_1157.barCount = -1

    bar_1158 = BarData()
    bar_1158.date = "1704067080"  # 11:58:00 timestamp
    bar_1158.open = 100.25
    bar_1158.high = 100.75
    bar_1158.low = 100.00
    bar_1158.close = 100.50
    bar_1158.volume = Decimal(1200)
    bar_1158.wap = Decimal(-1)
    bar_1158.barCount = -1

    bar_1159 = BarData()  # EOD bar that should be processed by timeout
    bar_1159.date = "1704067140"  # 11:59:00 timestamp
    bar_1159.open = 100.50
    bar_1159.high = 101.00
    bar_1159.low = 100.25
    bar_1159.close = 100.75
    bar_1159.volume = Decimal(800)
    bar_1159.wap = Decimal(-1)
    bar_1159.barCount = -1

    # Act - Process bars with a very short timeout for testing
    # Override the timeout method to use a shorter timeout

    async def short_timeout_schedule(bar_type_str, bar):
        # Use much shorter timeout for testing (0.1 seconds instead of 65 seconds)
        bar_type = BarType.from_str(bar_type_str)
        timeout_seconds = 0.1  # Short timeout for testing

        # Cancel any existing timeout task for this bar type
        if bar_type_str in ib_client._bar_timeout_tasks:
            ib_client._bar_timeout_tasks[bar_type_str].cancel()

        async def completion_handler():
            try:
                await asyncio.sleep(timeout_seconds)
                current_bar = ib_client._bar_type_to_last_bar.get(bar_type_str)
                if current_bar and int(current_bar.date) == int(bar.date):
                    ts_init = ib_client._clock.timestamp_ns()
                    nautilus_bar = await ib_client._ib_bar_to_nautilus_bar(
                        bar_type=bar_type,
                        bar=current_bar,
                        ts_init=ts_init,
                        is_revision=False,
                    )
                    if nautilus_bar and not (
                        nautilus_bar.is_single_price() and nautilus_bar.open.as_double() == 0
                    ):
                        await ib_client._handle_data(nautilus_bar)
            except asyncio.CancelledError:
                pass
            finally:
                ib_client._bar_timeout_tasks.pop(bar_type_str, None)

        task = asyncio.create_task(completion_handler())
        ib_client._bar_timeout_tasks[bar_type_str] = task

    ib_client._schedule_bar_completion_timeout = short_timeout_schedule

    # Process first bar (should return None, schedule timeout)
    result_1157 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1157,
        handle_revised_bars=False,
        historical=False,
    )

    # Process second bar (should return first bar immediately, schedule timeout for second)
    result_1158 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1158,
        handle_revised_bars=False,
        historical=False,
    )

    # Process third bar (should return second bar immediately, schedule timeout for third)
    result_1159 = await ib_client._process_bar_data(
        bar_type_str,
        bar_1159,
        handle_revised_bars=False,
        historical=False,
    )

    # Wait for timeout to trigger (should process the EOD bar)
    await asyncio.sleep(0.2)  # Wait longer than the timeout

    # Assert
    assert result_1157 is None  # First bar returns None (no previous bar)
    assert result_1158 is not None  # Second bar returns first bar immediately
    assert result_1159 is not None  # Third bar returns second bar immediately

    # The timeout should have processed the EOD bar (11:59)
    assert len(processed_bars) >= 1
    eod_bar = processed_bars[-1]  # Last processed bar should be the EOD bar
    assert eod_bar.ts_event == 1704067140000000000  # 11:59 bar timestamp
    assert eod_bar.close.as_double() == 100.75  # EOD bar close price


@pytest.mark.asyncio
async def test_process_bar_data_completion_timeout_cancellation(ib_client):
    """
    Test that completion timeout tasks are properly cancelled when new bars arrive.
    """
    # Arrange
    bar_type_str = "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    # Create test bars
    bar1 = BarData()
    bar1.date = "1704067020"
    bar1.open = 100.00
    bar1.high = 100.50
    bar1.low = 99.50
    bar1.close = 100.25
    bar1.volume = Decimal(1000)
    bar1.wap = Decimal(-1)
    bar1.barCount = -1

    bar2 = BarData()
    bar2.date = "1704067080"
    bar2.open = 100.25
    bar2.high = 100.75
    bar2.low = 100.00
    bar2.close = 100.50
    bar2.volume = Decimal(1200)
    bar2.wap = Decimal(-1)
    bar2.barCount = -1

    bar3 = BarData()
    bar3.date = "1704067140"
    bar3.open = 100.50
    bar3.high = 101.00
    bar3.low = 100.25
    bar3.close = 100.75
    bar3.volume = Decimal(800)
    bar3.wap = Decimal(-1)
    bar3.barCount = -1

    # Act - Process bars rapidly
    await ib_client._process_bar_data(bar_type_str, bar1, handle_revised_bars=False)
    await ib_client._process_bar_data(bar_type_str, bar2, handle_revised_bars=False)

    # Verify completion timeout task was created
    assert bar_type_str in ib_client._bar_timeout_tasks
    first_task = ib_client._bar_timeout_tasks[bar_type_str]
    assert not first_task.cancelled()

    # Process another bar quickly (should cancel previous timeout)
    await ib_client._process_bar_data(bar_type_str, bar3, handle_revised_bars=False)

    # Give a small delay for the cancellation to complete
    await asyncio.sleep(0.01)

    # Verify the first task was cancelled and a new one was created
    assert first_task.cancelled()
    assert bar_type_str in ib_client._bar_timeout_tasks
    second_task = ib_client._bar_timeout_tasks[bar_type_str]
    assert second_task != first_task
    assert not second_task.cancelled()


@pytest.mark.asyncio
async def test_process_bar_data_handle_revised_bars_true(ib_client):
    """
    Test that completion timeout mechanism is not used when handle_revised_bars=True.
    """
    # Arrange
    bar_type_str = "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    bar = BarData()
    bar.date = "1704067020"
    bar.open = 100.00
    bar.high = 100.50
    bar.low = 99.50
    bar.close = 100.25
    bar.volume = Decimal(1000)
    bar.wap = Decimal(-1)
    bar.barCount = -1

    # Act - Process bar with handle_revised_bars=True
    result = await ib_client._process_bar_data(
        bar_type_str,
        bar,
        handle_revised_bars=True,
        historical=False,
    )

    # Assert - Should return a bar immediately and not create completion timeout task
    assert result is not None
    assert bar_type_str not in ib_client._bar_timeout_tasks


@pytest.mark.asyncio
async def test_process_bar_data_out_of_sync_bars(ib_client):
    """
    Test that out-of-sync bars are handled correctly.
    """
    # Arrange
    bar_type_str = "AAPL.NASDAQ-1-MINUTE-LAST-EXTERNAL"
    ib_client._cache.add_instrument(IBTestContractStubs.aapl_instrument())

    # Create bars with timestamps out of order
    bar_newer = BarData()
    bar_newer.date = "1704067080"  # 11:58:00
    bar_newer.open = 100.25
    bar_newer.high = 100.75
    bar_newer.low = 100.00
    bar_newer.close = 100.50
    bar_newer.volume = Decimal(1200)
    bar_newer.wap = Decimal(-1)
    bar_newer.barCount = -1

    bar_older = BarData()
    bar_older.date = "1704067020"  # 11:57:00 (older than previous)
    bar_older.open = 100.00
    bar_older.high = 100.50
    bar_older.low = 99.50
    bar_older.close = 100.25
    bar_older.volume = Decimal(1000)
    bar_older.wap = Decimal(-1)
    bar_older.barCount = -1

    # Act - Process newer bar first, then older bar
    result1 = await ib_client._process_bar_data(bar_type_str, bar_newer, handle_revised_bars=False)
    result2 = await ib_client._process_bar_data(bar_type_str, bar_older, handle_revised_bars=False)

    # Assert - Out of sync bar should return None
    assert result1 is None  # First bar returns None
    assert result2 is None  # Out of sync bar returns None


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


@pytest.mark.skip(reason="Slow test - takes 60+ seconds")
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
