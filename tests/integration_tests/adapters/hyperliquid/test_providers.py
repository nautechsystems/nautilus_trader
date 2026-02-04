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

import pytest

from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig


class TestHyperliquidInstrumentProvider:
    def test_provider_initialization(self, mock_http_client):
        # Arrange & Act
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
        )

        # Assert
        assert provider is not None

    def test_provider_with_perp_only(self, mock_http_client):
        # Arrange & Act
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=[HyperliquidProductType.PERP],
        )

        # Assert
        assert provider is not None
        assert HyperliquidProductType.PERP in provider._product_types

    def test_provider_with_spot_only(self, mock_http_client):
        # Arrange & Act
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=[HyperliquidProductType.SPOT],
        )

        # Assert
        assert provider is not None
        assert HyperliquidProductType.SPOT in provider._product_types

    def test_provider_with_both_product_types(self, mock_http_client):
        # Arrange & Act
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=[HyperliquidProductType.PERP, HyperliquidProductType.SPOT],
        )

        # Assert
        assert provider is not None
        assert HyperliquidProductType.PERP in provider._product_types
        assert HyperliquidProductType.SPOT in provider._product_types

    def test_provider_with_empty_product_types_raises(self, mock_http_client):
        # Arrange & Act & Assert
        with pytest.raises(ValueError, match="product_types must contain at least one entry"):
            HyperliquidInstrumentProvider(
                client=mock_http_client,
                config=InstrumentProviderConfig(),
                product_types=[],
            )

    def test_provider_without_client_raises(self):
        # Arrange & Act & Assert
        with pytest.raises(TypeError):
            HyperliquidInstrumentProvider(
                client=None,
                config=InstrumentProviderConfig(),
            )

    @pytest.mark.asyncio
    async def test_load_all_async_calls_client(self, mock_http_client):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
        )

        # Act
        await provider.load_all_async()

        # Assert
        mock_http_client.load_instrument_definitions.assert_called_once()

    @pytest.mark.asyncio
    async def test_load_all_async_with_filters(self, mock_http_client):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
        )
        filters = {"symbol": "BTC"}

        # Act
        await provider.load_all_async(filters=filters)

        # Assert
        mock_http_client.load_instrument_definitions.assert_called_once()

    def test_instruments_pyo3_returns_list(self, mock_http_client):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
        )

        # Act
        result = provider.instruments_pyo3()

        # Assert
        assert isinstance(result, list)
