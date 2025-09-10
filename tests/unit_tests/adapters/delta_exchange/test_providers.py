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

"""
Unit tests for Delta Exchange instrument provider.
"""

import asyncio
import json
import os
import tempfile
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock, patch

import pytest

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeInstrumentProviderConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.common.component import LiveClock
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.model.currencies import Currency
from nautilus_trader.model.enums import AssetClass, InstrumentClass, OptionKind
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.model.instruments import CryptoPerpetual, CryptoOption
from nautilus_trader.model.objects import Price, Quantity
from nautilus_trader.test_kit.mocks import MockClock


class TestDeltaExchangeInstrumentProvider:
    """Test DeltaExchangeInstrumentProvider."""

    def setup_method(self):
        """Set up test fixtures."""
        self.clock = MockClock()
        self.client = MagicMock(spec=nautilus_pyo3.DeltaExchangeHttpClient)
        self.config = DeltaExchangeInstrumentProviderConfig()
        self.provider = DeltaExchangeInstrumentProvider(
            client=self.client,
            clock=self.clock,
            config=self.config,
        )

    def test_init(self):
        """Test provider initialization."""
        assert self.provider._client == self.client
        assert self.provider._clock == self.clock
        assert self.provider._config == self.config
        assert len(self.provider._instruments_pyo3) == 0
        assert len(self.provider._currencies) == 0
        assert self.provider._stats["total_loaded"] == 0

    def test_config_property(self):
        """Test config property."""
        assert self.provider.config == self.config

    def test_stats_property(self):
        """Test stats property."""
        stats = self.provider.stats
        assert isinstance(stats, dict)
        assert "total_loaded" in stats
        assert "filtered_out" in stats
        assert "cache_hits" in stats

    def test_instruments_pyo3(self):
        """Test instruments_pyo3 method."""
        instruments = self.provider.instruments_pyo3()
        assert isinstance(instruments, list)
        assert len(instruments) == 0

    @pytest.mark.asyncio
    async def test_load_all_async_with_cache_disabled(self):
        """Test loading all instruments with cache disabled."""
        # Mock API responses
        assets_response = {
            'result': [
                {'symbol': 'BTC', 'name': 'Bitcoin', 'precision': 8},
                {'symbol': 'ETH', 'name': 'Ethereum', 'precision': 8},
                {'symbol': 'USDT', 'name': 'Tether', 'precision': 6},
            ]
        }
        
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTCUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.5',
                    'contract_value': '1',
                    'min_size': '0.001',
                    'max_size': '1000000',
                    'trading_status': 'active',
                    'is_expired': False,
                    'initial_margin': '0.1',
                    'maintenance_margin': '0.05',
                    'maker_fee': '0.0005',
                    'taker_fee': '0.001',
                }
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        # Disable caching
        config = DeltaExchangeInstrumentProviderConfig(enable_instrument_caching=False)
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        await provider.load_all_async()

        # Verify API calls
        self.client.get_assets.assert_called_once()
        self.client.get_products.assert_called_once()

        # Verify instruments loaded
        assert len(provider.list_all()) == 1
        assert provider.stats["total_loaded"] == 1
        assert provider.stats["api_requests"] == 1

        # Verify instrument details
        instrument = provider.list_all()[0]
        assert isinstance(instrument, CryptoPerpetual)
        assert str(instrument.id.symbol) == "BTCUSD"
        assert instrument.id.venue == DELTA_EXCHANGE

    @pytest.mark.asyncio
    async def test_load_all_async_with_filters(self):
        """Test loading instruments with filters."""
        # Mock API responses
        assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTCUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.5',
                    'contract_value': '1',
                    'min_size': '0.001',
                    'trading_status': 'active',
                    'is_expired': False,
                },
                {
                    'id': 2,
                    'symbol': 'ETHUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'ETH',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.1',
                    'contract_value': '1',
                    'min_size': '0.01',
                    'trading_status': 'active',
                    'is_expired': False,
                },
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        # Configure with symbol filters
        config = DeltaExchangeInstrumentProviderConfig(
            symbol_filters=["BTC*"],
            enable_instrument_caching=False,
        )
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        await provider.load_all_async()

        # Should only load BTC instrument
        instruments = provider.list_all()
        assert len(instruments) == 1
        assert str(instruments[0].id.symbol) == "BTCUSD"
        assert provider.stats["total_loaded"] == 1
        assert provider.stats["filtered_out"] == 1

    @pytest.mark.asyncio
    async def test_load_all_async_with_product_type_filter(self):
        """Test loading instruments with product type filter."""
        # Mock API responses
        assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTCUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.5',
                    'contract_value': '1',
                    'min_size': '0.001',
                    'trading_status': 'active',
                    'is_expired': False,
                },
                {
                    'id': 2,
                    'symbol': 'BTC-25000-C',
                    'product_type': 'call_options',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'strike_price': '25000',
                    'expiry_time': 1640995200,  # Unix timestamp
                    'tick_size': '0.1',
                    'contract_value': '1',
                    'min_size': '0.1',
                    'trading_status': 'active',
                    'is_expired': False,
                },
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        # Configure to load only perpetual futures
        config = DeltaExchangeInstrumentProviderConfig(
            product_types=["perpetual_futures"],
            enable_instrument_caching=False,
        )
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        await provider.load_all_async()

        # Should only load perpetual future
        instruments = provider.list_all()
        assert len(instruments) == 1
        assert isinstance(instruments[0], CryptoPerpetual)
        assert str(instruments[0].id.symbol) == "BTCUSD"

    @pytest.mark.asyncio
    async def test_load_option_instrument(self):
        """Test loading option instruments."""
        # Mock API responses
        assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTC-25000-C',
                    'product_type': 'call_options',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'strike_price': '25000',
                    'expiry_time': 1640995200,
                    'tick_size': '0.1',
                    'contract_value': '1',
                    'min_size': '0.1',
                    'trading_status': 'active',
                    'is_expired': False,
                    'initial_margin': '0.2',
                    'maintenance_margin': '0.1',
                    'maker_fee': '0.001',
                    'taker_fee': '0.002',
                }
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        config = DeltaExchangeInstrumentProviderConfig(
            product_types=["call_options"],
            enable_instrument_caching=False,
        )
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        await provider.load_all_async()

        # Verify option instrument
        instruments = provider.list_all()
        assert len(instruments) == 1
        
        option = instruments[0]
        assert isinstance(option, CryptoOption)
        assert str(option.id.symbol) == "BTC-25000-C"
        assert option.option_kind == OptionKind.CALL
        assert option.strike_price == Price(25000, 1)
        assert option.expiry_ns == 1640995200 * 1_000_000_000

    @pytest.mark.asyncio
    async def test_load_ids_async(self):
        """Test loading specific instrument IDs."""
        # Mock API responses
        assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTCUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.5',
                    'contract_value': '1',
                    'min_size': '0.001',
                    'trading_status': 'active',
                    'is_expired': False,
                },
                {
                    'id': 2,
                    'symbol': 'ETHUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'ETH',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.1',
                    'contract_value': '1',
                    'min_size': '0.01',
                    'trading_status': 'active',
                    'is_expired': False,
                },
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        config = DeltaExchangeInstrumentProviderConfig(enable_instrument_caching=False)
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        # Load specific instrument ID
        btc_id = InstrumentId(Symbol("BTCUSD"), DELTA_EXCHANGE)
        await provider.load_ids_async([btc_id])

        # Should only have the requested instrument
        instruments = provider.list_all()
        assert len(instruments) == 1
        assert instruments[0].id == btc_id

    @pytest.mark.asyncio
    async def test_load_async_single_instrument(self):
        """Test loading a single instrument."""
        # Mock API responses
        assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
        products_response = {
            'result': [
                {
                    'id': 1,
                    'symbol': 'BTCUSD',
                    'product_type': 'perpetual_futures',
                    'underlying_asset': 'BTC',
                    'quoting_asset': 'USD',
                    'settlement_asset': 'USDT',
                    'tick_size': '0.5',
                    'contract_value': '1',
                    'min_size': '0.001',
                    'trading_status': 'active',
                    'is_expired': False,
                }
            ]
        }

        self.client.get_assets = AsyncMock(return_value=assets_response)
        self.client.get_products = AsyncMock(return_value=products_response)

        config = DeltaExchangeInstrumentProviderConfig(enable_instrument_caching=False)
        provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

        # Load single instrument
        btc_id = InstrumentId(Symbol("BTCUSD"), DELTA_EXCHANGE)
        await provider.load_async(btc_id)

        # Verify instrument loaded
        instruments = provider.list_all()
        assert len(instruments) == 1
        assert instruments[0].id == btc_id

    def test_parse_decimal(self):
        """Test decimal parsing utility."""
        assert self.provider._parse_decimal("123.456") == Decimal("123.456")
        assert self.provider._parse_decimal(123.456) == Decimal("123.456")
        assert self.provider._parse_decimal(None) == Decimal("0")
        assert self.provider._parse_decimal("invalid") == Decimal("0")

    def test_calculate_precision(self):
        """Test precision calculation utility."""
        assert self.provider._calculate_precision(Decimal("0.001")) == 3
        assert self.provider._calculate_precision(Decimal("0.5")) == 1
        assert self.provider._calculate_precision(Decimal("1")) == 0
        assert self.provider._calculate_precision(Decimal("0")) == 8

    def test_get_currency(self):
        """Test currency retrieval and creation."""
        # Test creating new currency
        btc = self.provider._get_currency("BTC")
        assert btc is not None
        assert btc.code == "BTC"
        assert btc.precision == 8

        # Test retrieving existing currency
        btc2 = self.provider._get_currency("btc")  # lowercase
        assert btc2 == btc

        # Test None input
        none_currency = self.provider._get_currency(None)
        assert none_currency is None

    def test_should_process_product(self):
        """Test product filtering logic."""
        product = {
            'symbol': 'BTCUSD',
            'product_type': 'perpetual_futures',
            'trading_status': 'active',
            'is_expired': False,
        }

        # Should process with default config
        assert self.provider._should_process_product(product, ['perpetual_futures'], None)

        # Should not process wrong product type
        assert not self.provider._should_process_product(product, ['call_options'], None)

        # Should not process with symbol filter mismatch
        filters = {'symbol_filters': ['ETH*']}
        assert not self.provider._should_process_product(product, ['perpetual_futures'], filters)

        # Should process with matching symbol filter
        filters = {'symbol_filters': ['BTC*']}
        assert self.provider._should_process_product(product, ['perpetual_futures'], filters)

    @pytest.mark.asyncio
    async def test_caching_functionality(self):
        """Test instrument caching functionality."""
        with tempfile.TemporaryDirectory() as temp_dir:
            # Configure with caching enabled
            config = DeltaExchangeInstrumentProviderConfig(
                enable_instrument_caching=True,
                cache_directory=temp_dir,
                cache_validity_hours=1,
            )
            provider = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

            # Mock API responses
            assets_response = {'result': [{'symbol': 'BTC', 'precision': 8}]}
            products_response = {
                'result': [
                    {
                        'id': 1,
                        'symbol': 'BTCUSD',
                        'product_type': 'perpetual_futures',
                        'underlying_asset': 'BTC',
                        'quoting_asset': 'USD',
                        'settlement_asset': 'USDT',
                        'tick_size': '0.5',
                        'contract_value': '1',
                        'min_size': '0.001',
                        'trading_status': 'active',
                        'is_expired': False,
                    }
                ]
            }

            self.client.get_assets = AsyncMock(return_value=assets_response)
            self.client.get_products = AsyncMock(return_value=products_response)

            # First load - should hit API and save to cache
            await provider.load_all_async()
            assert provider.stats["api_requests"] == 1
            assert provider.stats["cache_hits"] == 0

            # Verify cache file exists
            cache_path = config.get_cache_file_path()
            assert os.path.exists(cache_path)

            # Create new provider instance
            provider2 = DeltaExchangeInstrumentProvider(self.client, self.clock, config)

            # Second load - should hit cache
            await provider2.load_all_async()
            assert provider2.stats["cache_hits"] == 1
            assert len(provider2.list_all()) == 1

    @pytest.mark.asyncio
    async def test_error_handling(self):
        """Test error handling in various scenarios."""
        # Test API error
        self.client.get_assets = AsyncMock(side_effect=Exception("API Error"))
        
        with pytest.raises(RuntimeError, match="Failed to load Delta Exchange instruments"):
            await self.provider.load_all_async()

        # Test invalid instrument ID venue
        invalid_id = InstrumentId(Symbol("BTCUSD"), "INVALID_VENUE")
        
        with pytest.raises(ValueError, match="Invalid instrument ID venue"):
            await self.provider.load_ids_async([invalid_id])

    def test_create_instrument_info(self):
        """Test instrument info creation."""
        product = {
            'id': 123,
            'description': 'BTC Perpetual',
            'product_type': 'perpetual_futures',
            'trading_status': 'active',
            'is_expired': False,
        }

        info = self.provider._create_instrument_info(product)
        assert info['id'] == 123
        assert info['description'] == 'BTC Perpetual'
        assert info['product_type'] == 'perpetual_futures'
        assert info['trading_status'] == 'active'
        assert not info['is_expired']
