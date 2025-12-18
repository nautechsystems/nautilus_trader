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

import pytest

from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.nautilus_pyo3 import DeribitInstrumentKind
from nautilus_trader.model.enums import BookType
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
