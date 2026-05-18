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

from decimal import Decimal

import pytest

from nautilus_trader.adapters.hyperliquid.constants import HYPERLIQUID_VENUE
from nautilus_trader.adapters.hyperliquid.enums import HyperliquidProductType
from nautilus_trader.adapters.hyperliquid.providers import HyperliquidInstrumentProvider
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.model.currencies import USDC
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.instruments import BinaryOption
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


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

    @pytest.mark.parametrize(
        ("product_types", "expected_kwargs"),
        [
            (
                [HyperliquidProductType.OUTCOME],
                {
                    "include_spot": False,
                    "include_perps": False,
                    "include_perps_hip3": False,
                    "include_outcomes": True,
                },
            ),
            (
                [HyperliquidProductType.PERP],
                {
                    "include_spot": False,
                    "include_perps": True,
                    "include_perps_hip3": False,
                    "include_outcomes": False,
                },
            ),
            (
                [HyperliquidProductType.SPOT, HyperliquidProductType.OUTCOME],
                {
                    "include_spot": True,
                    "include_perps": False,
                    "include_perps_hip3": False,
                    "include_outcomes": True,
                },
            ),
        ],
    )
    @pytest.mark.asyncio
    async def test_load_all_async_passes_include_outcomes(
        self,
        mock_http_client,
        product_types,
        expected_kwargs,
    ):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=product_types,
        )

        # Act
        await provider.load_all_async()

        # Assert
        mock_http_client.load_instrument_definitions.assert_called_once_with(**expected_kwargs)

    def test_instrument_product_type_recognizes_binary_option(self, mock_http_client):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=[HyperliquidProductType.OUTCOME],
        )
        binary_option = _make_outcome_binary_option()

        # Act
        product_type = provider._instrument_product_type(binary_option)

        # Assert
        assert product_type is HyperliquidProductType.OUTCOME

    @pytest.mark.parametrize(
        ("filters", "expected"),
        [
            ({"market_types": ["outcome"]}, True),
            ({"market_types": ["spot", "perp"]}, False),
            ({"market_types": ["outcome", "spot"]}, True),
            ({}, True),
            (None, True),
        ],
    )
    def test_accept_instrument_outcome_market_type_filter(
        self,
        mock_http_client,
        filters,
        expected,
    ):
        # Arrange
        provider = HyperliquidInstrumentProvider(
            client=mock_http_client,
            config=InstrumentProviderConfig(),
            product_types=[HyperliquidProductType.OUTCOME],
        )
        binary_option = _make_outcome_binary_option()

        # Act
        accepted = provider._accept_instrument(binary_option, filters)

        # Assert
        assert accepted is expected


def _make_outcome_binary_option() -> BinaryOption:
    instrument_id = InstrumentId(Symbol("+50"), HYPERLIQUID_VENUE)
    price_increment = Price.from_str("0.0001")
    size_increment = Quantity.from_str("0.01")
    return BinaryOption(
        instrument_id=instrument_id,
        raw_symbol=Symbol("#50"),
        outcome="Yes",
        description="class:priceBinary|underlying:BTC|expiry:20260508-0600",
        asset_class=AssetClass.ALTERNATIVE,
        currency=USDC,
        price_precision=price_increment.precision,
        price_increment=price_increment,
        size_precision=size_increment.precision,
        size_increment=size_increment,
        activation_ns=0,
        expiration_ns=0,
        max_quantity=None,
        min_quantity=Quantity.from_int(1),
        maker_fee=Decimal(0),
        taker_fee=Decimal(0),
        ts_event=0,
        ts_init=0,
    )
