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
from unittest.mock import Mock
from unittest.mock import patch

import pandas as pd
import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import LiveDataEngineConfig
from nautilus_trader.core.data import Data
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.messages import DataResponse
from nautilus_trader.data.messages import RequestQuoteTicks
from nautilus_trader.data.messages import SubscribeData
from nautilus_trader.live.data_engine import LiveDataEngine
from nautilus_trader.model.data import DataType
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.portfolio.portfolio import Portfolio
from nautilus_trader.test_kit.functions import eventually
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.data import TestDataStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


BITMEX = Venue("BITMEX")
BINANCE = Venue("BINANCE")
XBTUSD_BITMEX = TestInstrumentProvider.xbtusd_bitmex()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
ETHUSDT_BINANCE = TestInstrumentProvider.ethusdt_binance()


class TestLiveDataEngine:
    @pytest.fixture(autouse=True)
    def setup(self, request):
        # Fixture Setup - get the event loop that pytest-asyncio will use for tests
        self.loop = request.getfixturevalue("event_loop")
        self.loop.set_debug(True)

        self.clock = LiveClock()

        self.trader_id = TestIdStubs.trader_id()

        self.msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        self.cache = TestComponentStubs.cache()

        self.portfolio = Portfolio(
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
        )

        yield

        # Teardown - only dispose, ensure_all_tasks_completed() will fail with closed loop
        self.engine.dispose()

    @pytest.mark.asyncio
    async def test_start_when_loop_not_running_logs(self):
        # Arrange, Act
        self.engine.start()

        # Assert
        assert True  # No exceptions raised
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_put_data_command(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveDataEngineConfig(qsize=1),
        )

        subscribe = SubscribeData(
            instrument_id=None,
            client_id=None,
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)
        self.engine.execute(subscribe)

        # Assert
        await eventually(lambda: self.engine.cmd_qsize() == 1)
        assert self.engine.command_count == 0

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_send_request(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveDataEngineConfig(qsize=1),
        )

        handler = []
        request = request = RequestQuoteTicks(
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=ClientId("RANDOM"),
            venue=None,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.engine.request(request)
        self.engine.request(request)

        # Assert
        await eventually(lambda: self.engine.req_qsize() == 1)
        assert self.engine.command_count == 0

    @pytest.mark.asyncio
    async def test_message_qsize_at_max_blocks_on_receive_response(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveDataEngineConfig(qsize=1),
        )

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            start=pd.Timestamp("2023-01-01"),
            end=pd.Timestamp("2023-01-02"),
        )

        # Act
        self.engine.response(response)
        self.engine.response(response)  # Add over max size

        # Assert
        await eventually(lambda: self.engine.res_qsize() == 1)
        assert self.engine.command_count == 0

    @pytest.mark.asyncio
    async def test_data_qsize_at_max_blocks_on_put_data(self):
        # Arrange
        self.msgbus.deregister(endpoint="DataEngine.execute", handler=self.engine.execute)
        self.msgbus.deregister(endpoint="DataEngine.process", handler=self.engine.process)
        self.msgbus.deregister(endpoint="DataEngine.request", handler=self.engine.request)
        self.msgbus.deregister(endpoint="DataEngine.response", handler=self.engine.response)

        self.engine = LiveDataEngine(
            loop=self.loop,
            msgbus=self.msgbus,
            cache=self.cache,
            clock=self.clock,
            config=LiveDataEngineConfig(qsize=1),
        )

        data = Data(1_000_000_000, 1_000_000_000)

        # Act
        self.engine.process(data)
        self.engine.process(data)  # Add over max size

        # Assert
        await eventually(lambda: self.engine.data_qsize() == 1)
        assert self.engine.data_count == 0

    @pytest.mark.asyncio
    async def test_start(self):
        # Arrange, Act
        self.engine.start()

        # Assert
        await eventually(lambda: self.engine.is_running)

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_kill_when_running_and_no_messages_on_queues(self):
        # Arrange, Act
        self.engine.start()
        await eventually(lambda: self.engine.is_running)
        self.engine.kill()

        # Assert
        assert self.engine.is_stopped

    @pytest.mark.asyncio
    async def test_kill_when_not_running_with_messages_on_queue(self):
        # Arrange, Act
        self.engine.kill()

        # Assert
        assert self.engine.data_qsize() == 0

    @pytest.mark.asyncio
    async def test_execute_command_processes_message(self):
        # Arrange
        self.engine.start()

        subscribe = SubscribeData(
            instrument_id=None,
            client_id=None,
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            command_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
        )

        # Act
        self.engine.execute(subscribe)

        # Assert
        await eventually(lambda: self.engine.cmd_qsize() == 0)
        await eventually(lambda: self.engine.command_count == 1)

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_send_request_processes_message(self):
        # Arrange
        self.engine.start()

        handler = []
        request = RequestQuoteTicks(
            instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
            start=None,
            end=None,
            limit=1000,
            client_id=ClientId("RANDOM"),
            venue=None,
            callback=handler.append,
            request_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            params=None,
        )

        # Act
        self.engine.request(request)

        # Assert
        await eventually(lambda: self.engine.req_qsize() == 0)
        await eventually(lambda: self.engine.request_count == 1)

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_receive_response_processes_message(self):
        # Arrange
        self.engine.start()

        response = DataResponse(
            client_id=ClientId("BINANCE"),
            venue=BINANCE,
            data_type=DataType(QuoteTick),
            data=[],
            correlation_id=UUID4(),
            response_id=UUID4(),
            ts_init=self.clock.timestamp_ns(),
            start=pd.Timestamp("2023-01-01"),
            end=pd.Timestamp("2023-01-02"),
        )

        # Act
        self.engine.response(response)

        # Assert
        await eventually(lambda: self.engine.res_qsize() == 0)
        await eventually(lambda: self.engine.response_count == 1)

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_process_data_processes_data(self):
        # Arrange
        self.engine.start()

        # Act
        tick = TestDataStubs.trade_tick()

        # Act
        self.engine.process(tick)

        # Assert
        await eventually(lambda: self.engine.data_qsize() == 0)
        await eventually(lambda: self.engine.data_count == 1)

        # Tear Down
        self.engine.stop()

    @pytest.mark.asyncio
    async def test_graceful_shutdown_on_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _handle_data to raise an exception
        def mock_handle_data(data):
            raise ValueError("Test exception for graceful shutdown")

        with patch.object(engine, "_handle_data", side_effect=mock_handle_data):
            engine.start()

            # Act - Send data that will trigger the exception
            test_data = TestDataStubs.trade_tick()
            engine.process(test_data)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            # Wait for queue to empty
            await eventually(lambda: engine.data_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_on_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on
        exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock os._exit to track calls instead of actually exiting
        with patch("os._exit") as exit_mock:
            # Mock _handle_data to raise an exception
            def mock_handle_data(data):
                raise ValueError("Test exception for immediate crash")

            with patch.object(engine, "_handle_data", side_effect=mock_handle_data):
                engine.start()

                # Act - Send data that will trigger the exception
                test_data = TestDataStubs.trade_tick()
                engine.process(test_data)

                # Wait for processing and os._exit call
                await eventually(lambda: exit_mock.called)

                # Assert
                exit_mock.assert_called_once_with(1)

            engine.stop()

            await eventually(lambda: engine.data_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_only_called_once_on_repeated_exceptions(self):
        """
        Test that shutdown_system is only called once even with repeated exceptions.
        """
        # Arrange
        # Create fresh msgbus to avoid endpoint conflicts
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _handle_data to raise an exception
        def mock_handle_data(data):
            raise ValueError("Repeated exception")

        with patch.object(engine, "_handle_data", side_effect=mock_handle_data):
            engine.start()

            # Act - Send multiple data items that will trigger exceptions
            test_data = TestDataStubs.trade_tick()

            engine.process(test_data)
            await eventually(lambda: shutdown_mock.called)  # Wait for first shutdown call

            engine.process(test_data)  # Second exception
            engine.process(test_data)  # Third exception

            # Give a moment for any potential additional calls (should not happen)
            await asyncio.sleep(0.1)

            # Assert - shutdown_system should only be called once
            assert shutdown_mock.call_count == 1
            assert engine._shutdown_initiated is True

            engine.stop()

            await eventually(lambda: engine.data_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_cmd_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        DataCommand queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _execute_command to raise an exception
        def mock_execute_command(command):
            raise ValueError("Test exception for graceful shutdown in cmd queue")

        with patch.object(engine, "_execute_command", side_effect=mock_execute_command):
            engine.start()

            # Act - Send command that will trigger the exception
            subscribe = SubscribeData(
                instrument_id=None,
                client_id=None,
                venue=BINANCE,
                data_type=DataType(QuoteTick),
                command_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
            )
            engine.execute(subscribe)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown in cmd queue" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            await eventually(lambda: engine.cmd_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_req_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        RequestData queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _handle_request to raise an exception
        def mock_handle_request(request):
            raise ValueError("Test exception for graceful shutdown in req queue")

        with patch.object(engine, "_handle_request", side_effect=mock_handle_request):
            engine.start()

            # Act - Send request that will trigger the exception
            handler = []
            request = RequestQuoteTicks(
                instrument_id=InstrumentId(Symbol("SOMETHING"), Venue("RANDOM")),
                start=None,
                end=None,
                limit=1000,
                client_id=ClientId("RANDOM"),
                venue=None,
                callback=handler.append,
                request_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                params=None,
            )
            engine.request(request)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown in req queue" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            await eventually(lambda: engine.req_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_res_queue_exception_enabled_calls_shutdown_system(self):
        """
        Test that when graceful_shutdown_on_exception=True, shutdown_system is called on
        DataResponse queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=True)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock shutdown_system to track calls
        shutdown_mock = Mock()
        engine.shutdown_system = shutdown_mock

        # Mock _handle_response to raise an exception
        def mock_handle_response(response):
            raise ValueError("Test exception for graceful shutdown in res queue")

        with patch.object(engine, "_handle_response", side_effect=mock_handle_response):
            engine.start()

            # Act - Send response that will trigger the exception
            response = DataResponse(
                client_id=ClientId("BINANCE"),
                venue=BINANCE,
                data_type=DataType(QuoteTick),
                data=[],
                correlation_id=UUID4(),
                response_id=UUID4(),
                ts_init=self.clock.timestamp_ns(),
                start=pd.Timestamp("2023-01-01"),
                end=pd.Timestamp("2023-01-02"),
            )
            engine.response(response)

            # Wait for processing and shutdown call
            await eventually(lambda: shutdown_mock.called)

            # Assert
            shutdown_mock.assert_called_once()
            args = shutdown_mock.call_args[0]
            assert "Test exception for graceful shutdown in res queue" in args[0]
            assert engine._shutdown_initiated is True

            engine.stop()
            await eventually(lambda: engine.res_qsize() == 0)

    @pytest.mark.asyncio
    async def test_graceful_shutdown_data_queue_exception_disabled_calls_os_exit(self):
        """
        Test that when graceful_shutdown_on_exception=False, os._exit is called on data
        queue exception.
        """
        # Arrange
        test_msgbus = MessageBus(
            trader_id=self.trader_id,
            clock=self.clock,
        )

        config = LiveDataEngineConfig(graceful_shutdown_on_exception=False)
        engine = LiveDataEngine(
            loop=self.loop,
            msgbus=test_msgbus,
            cache=self.cache,
            clock=self.clock,
            config=config,
        )

        # Mock os._exit to track calls instead of actually exiting
        with patch("os._exit") as exit_mock:
            # Mock _handle_data to raise an exception
            def mock_handle_data(data):
                raise ValueError("Test exception for immediate crash in data queue")

            with patch.object(engine, "_handle_data", side_effect=mock_handle_data):
                engine.start()

                # Act - Send data that will trigger the exception
                test_data = TestDataStubs.trade_tick()
                engine.process(test_data)

                # Wait for processing and os._exit call
                await eventually(lambda: exit_mock.called)

                # Assert
                exit_mock.assert_called_once_with(1)

            engine.stop()
            await eventually(lambda: engine.data_qsize() == 0)
