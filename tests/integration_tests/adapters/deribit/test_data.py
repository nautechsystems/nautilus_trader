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

from types import SimpleNamespace
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


class TestDeribitDataClient:
    @pytest.mark.asyncio
    async def test_connect_and_disconnect_manage_resources(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        # Act
        await client._connect()

        try:
            # Assert
            client._instrument_provider.initialize.assert_called_once()
            client._ws_client.connect.assert_called_once()
            client._ws_client.wait_until_active.assert_called_once_with(timeout_secs=30.0)
        finally:
            # Act: Disconnect
            await client._disconnect()
            # Assert
            client._ws_client.close.assert_called_once()

    @pytest.mark.asyncio
    async def test_subscribe_order_book_deltas(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.subscribe_book.reset_mock()

            command = SimpleNamespace(
                book_type=BookType.L2_MBP,
                depth=None,
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._subscribe_order_book_deltas(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_book.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_quote_ticks(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.subscribe_quotes.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._subscribe_quote_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_quotes.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_trade_ticks(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.subscribe_trades.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._subscribe_trade_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_trades.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_quote_ticks(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.unsubscribe_quotes.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._unsubscribe_quote_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_quotes.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_trade_ticks(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.unsubscribe_trades.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._unsubscribe_trade_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_trades.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_order_book_deltas(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.unsubscribe_book.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
            )

            # Act
            await client._unsubscribe_order_book_deltas(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_book.assert_called_once_with(expected_id)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_multiple_instrument_kinds(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder(
            instrument_kinds=(
                DeribitInstrumentKind.FUTURE,
                DeribitInstrumentKind.OPTION,
            ),
        )
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        # Act
        await client._connect()

        try:
            # Assert
            assert client._config.instrument_kinds == (
                DeribitInstrumentKind.FUTURE,
                DeribitInstrumentKind.OPTION,
            )
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_request_instrument_receives_single_instrument(
        self,
        data_client_builder,
        instrument,
        live_clock,
        mock_http_client,
    ) -> None:
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {instrument.id: instrument}

        # Use the client's internal msgbus for subscription
        msgbus = client._msgbus

        # Create DataEngine using the client's msgbus
        data_engine = DataEngine(msgbus, client._cache, live_clock)
        data_engine.register_client(client)
        data_engine.start()

        # Track received instruments via msgbus subscription
        received_instruments: list = []
        topic = f"data.instrument.{instrument.id.venue}.{instrument.id.symbol}"
        msgbus.subscribe(topic=topic, handler=received_instruments.append)

        # Mock HTTP client to return a pyo3 instrument
        mock_pyo3_instrument = MagicMock()
        mock_http_client.request_instrument.return_value = mock_pyo3_instrument

        try:
            # Patch transform_instrument_from_pyo3 to return our fixture instrument
            with patch(
                "nautilus_trader.adapters.deribit.data.transform_instrument_from_pyo3",
                return_value=instrument,
            ):
                # Create request
                request = RequestInstrument(
                    instrument_id=instrument.id,
                    start=None,
                    end=None,
                    client_id=ClientId(DERIBIT_VENUE.value),
                    venue=DERIBIT_VENUE,
                    callback=lambda x: None,
                    request_id=UUID4(),
                    ts_init=live_clock.timestamp_ns(),
                    params=None,
                )

                # Act - Request the instrument
                await client._request_instrument(request)

            # Assert - Should receive exactly ONE instrument (not 2!)
            assert len(received_instruments) == 1, (
                f"Expected 1 instrument publication, got {len(received_instruments)}. "
                f"This indicates duplicate publication bug! "
                f"Received: {received_instruments}"
            )
            assert received_instruments[0].id == instrument.id
        finally:
            data_engine.stop()
            await client._disconnect()
