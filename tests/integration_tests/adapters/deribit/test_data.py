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

from types import SimpleNamespace
from unittest.mock import MagicMock
from unittest.mock import patch

import pytest

import nautilus_trader.adapters.deribit.data as deribit_data_module
from nautilus_trader.adapters.deribit.constants import DERIBIT_VENUE
from nautilus_trader.adapters.deribit.data import DeribitVolatilityIndex
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.data import Data
from nautilus_trader.core.nautilus_pyo3 import DeribitProductType
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.data.engine import DataEngine
from nautilus_trader.data.messages import RequestInstrument
from nautilus_trader.model.data import CustomData
from nautilus_trader.model.data import DataType
from nautilus_trader.model.enums import BookType
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol


class _FakePyo3DeribitVolatilityIndex:
    def __init__(
        self,
        index_name: str,
        volatility: float,
        ts_event: int = 0,
        ts_init: int = 0,
    ) -> None:
        self.index_name = index_name
        self.volatility = volatility
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        return self._ts_event

    @property
    def ts_init(self) -> int:
        return self._ts_init


class _FakePyo3DataType:
    def __init__(self, metadata: dict[str, str]):
        self.metadata = metadata


class _FakePyo3CustomData:
    def __init__(self, data: _FakePyo3DeribitVolatilityIndex, data_type: _FakePyo3DataType):
        self.data = data
        self.data_type = data_type


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
                params=None,
            )

            # Act
            await client._subscribe_order_book_deltas(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_book.assert_called_once_with(expected_id, None, None)
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
                params=None,
            )

            # Act
            await client._subscribe_trade_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_trades.assert_called_once_with(expected_id, None)
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
                params=None,
            )

            # Act
            await client._unsubscribe_trade_ticks(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_trades.assert_called_once_with(expected_id, None)
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
                params=None,
            )

            # Act
            await client._unsubscribe_order_book_deltas(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_book.assert_called_once_with(expected_id, None, None)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_mark_prices(
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
            client._ws_client.subscribe_mark_prices.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
                params=None,
            )

            # Act
            await client._subscribe_mark_prices(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_mark_prices.assert_called_once_with(expected_id, None)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_index_prices(
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
            client._ws_client.subscribe_index_prices.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
                params=None,
            )

            # Act
            await client._subscribe_index_prices(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.subscribe_index_prices.assert_called_once_with(expected_id, None)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_mark_prices(
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
            client._ws_client.unsubscribe_mark_prices.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
                params=None,
            )

            # Act
            await client._unsubscribe_mark_prices(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_mark_prices.assert_called_once_with(expected_id, None)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_index_prices(
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
            client._ws_client.unsubscribe_index_prices.reset_mock()

            command = SimpleNamespace(
                instrument_id=InstrumentId(Symbol("BTC-PERPETUAL"), DERIBIT_VENUE),
                params=None,
            )

            # Act
            await client._unsubscribe_index_prices(command)

            # Assert
            expected_id = nautilus_pyo3.InstrumentId.from_str("BTC-PERPETUAL.DERIBIT")
            client._ws_client.unsubscribe_index_prices.assert_called_once_with(expected_id, None)
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_subscribe_custom_data_volatility_index(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.subscribe_volatility_index.reset_mock()

            command = SimpleNamespace(
                data_type=DataType(
                    type=DeribitVolatilityIndex,
                    metadata={"index_name": "btc_usd"},
                ),
            )

            await client._subscribe(command)

            client._ws_client.subscribe_volatility_index.assert_called_once_with("btc_usd")
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_unsubscribe_custom_data_volatility_index(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        client = data_client_builder()
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        await client._connect()

        try:
            client._ws_client.unsubscribe_volatility_index.reset_mock()

            command = SimpleNamespace(
                data_type=DataType(
                    type=DeribitVolatilityIndex,
                    metadata={"index_name": "eth_usd"},
                ),
            )

            await client._unsubscribe(command)

            client._ws_client.unsubscribe_volatility_index.assert_called_once_with("eth_usd")
        finally:
            await client._disconnect()

    @pytest.mark.asyncio
    async def test_handle_msg_custom_data_volatility_index_forwarded(
        self,
        data_client_builder,
        monkeypatch,
    ) -> None:
        client = data_client_builder()
        client._handle_data = MagicMock()

        monkeypatch.setattr(
            deribit_data_module,
            "_PYO3DeribitVolatilityIndex",
            _FakePyo3DeribitVolatilityIndex,
        )
        monkeypatch.setattr(
            deribit_data_module.nautilus_pyo3,
            "CustomData",
            _FakePyo3CustomData,
        )

        dvol = _FakePyo3DeribitVolatilityIndex(
            index_name="btc_usd",
            volatility=72.5,
            ts_event=1_000,
            ts_init=1_001,
        )
        msg = _FakePyo3CustomData(
            data=dvol,
            data_type=_FakePyo3DataType({"index_name": "btc_usd"}),
        )

        client._handle_msg(msg)

        client._handle_data.assert_called_once()
        forwarded = client._handle_data.call_args.args[0]
        assert isinstance(forwarded, CustomData)
        assert isinstance(forwarded.data, Data)
        assert isinstance(forwarded.data, DeribitVolatilityIndex)
        assert forwarded.data.index_name == "btc_usd"
        assert forwarded.data.volatility == 72.5
        assert forwarded.data_type == DataType(
            DeribitVolatilityIndex,
            {"index_name": "btc_usd"},
        )

    @pytest.mark.asyncio
    async def test_subscribe_multiple_product_types(
        self,
        data_client_builder,
        instrument,
    ) -> None:
        # Arrange
        client = data_client_builder(
            product_types=(
                DeribitProductType.FUTURE,
                DeribitProductType.OPTION,
            ),
        )
        client._instrument_provider.get_all.return_value = {
            instrument.id: instrument,
        }

        # Act
        await client._connect()

        try:
            # Assert
            assert client._config.product_types == (
                DeribitProductType.FUTURE,
                DeribitProductType.OPTION,
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
                f"Expected 1 instrument publication, was {len(received_instruments)}. "
                f"This indicates duplicate publication bug! "
                f"Received: {received_instruments}"
            )
            assert received_instruments[0].id == instrument.id
        finally:
            data_engine.stop()
            await client._disconnect()
