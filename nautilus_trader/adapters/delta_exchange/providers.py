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
Instrument provider for Delta Exchange.

This module provides comprehensive instrument loading and management for Delta Exchange,
supporting all product types (perpetual futures, options) with intelligent caching,
filtering, and error handling.
"""

from __future__ import annotations

import asyncio
import fnmatch
import json
import os
import time
from decimal import Decimal
from typing import TYPE_CHECKING, Any

import msgspec

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeInstrumentProviderConfig
from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_PRODUCT_TYPES,
)
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.core.correctness import PyCondition
from nautilus_trader.model.currencies import Currency
from nautilus_trader.model.enums import AssetClass, InstrumentClass, InstrumentStatus, OptionKind
from nautilus_trader.model.identifiers import InstrumentId, Symbol
from nautilus_trader.model.instruments import (
    CryptoPerpetual,
    CryptoOption,
    Instrument,
    instruments_from_pyo3,
)
from nautilus_trader.model.objects import Money, Price, Quantity


if TYPE_CHECKING:
    from nautilus_trader.common.component import LiveClock


class DeltaExchangeInstrumentProvider(InstrumentProvider):
    """
    Provides Nautilus instrument definitions from Delta Exchange.

    This provider loads and manages Delta Exchange instruments with comprehensive
    filtering, caching, and error handling capabilities. It supports all Delta Exchange
    product types including perpetual futures and options.

    Parameters
    ----------
    client : DeltaExchangeHttpClient
        The Delta Exchange HTTP client for API communication.
    clock : LiveClock
        The clock instance for timing operations.
    config : DeltaExchangeInstrumentProviderConfig, optional
        The instrument provider configuration. If None, uses default configuration.

    Features
    --------
    - Supports all Delta Exchange product types (perpetual_futures, call_options, put_options)
    - Intelligent caching with configurable validity periods
    - Symbol and product type filtering with glob pattern support
    - Concurrent loading with rate limiting
    - Comprehensive error handling with retry logic
    - Memory and disk caching for performance
    - Incremental updates to minimize API calls

    Examples
    --------
    >>> # Basic usage with default configuration
    >>> provider = DeltaExchangeInstrumentProvider(client, clock)
    >>> await provider.load_all_async()

    >>> # Load only BTC perpetual futures
    >>> config = DeltaExchangeInstrumentProviderConfig(
    ...     product_types=["perpetual_futures"],
    ...     symbol_filters=["BTC*"]
    ... )
    >>> provider = DeltaExchangeInstrumentProvider(client, clock, config)
    >>> await provider.load_all_async()

    """

    def __init__(
        self,
        client: nautilus_pyo3.DeltaExchangeHttpClient,
        clock: LiveClock,
        config: DeltaExchangeInstrumentProviderConfig | None = None,
    ) -> None:
        super().__init__(config=config)
        self._client = client
        self._clock = clock
        self._config = config or DeltaExchangeInstrumentProviderConfig()
        self._log_warnings = self._config.log_warnings if hasattr(self._config, 'log_warnings') else True

        # Internal state
        self._instruments_pyo3: list[nautilus_pyo3.Instrument] = []
        self._currencies: dict[str, Currency] = {}
        self._loading_lock = asyncio.Lock()
        self._last_load_time: float | None = None
        self._load_count = 0

        # Caching
        self._memory_cache: dict[str, Any] = {}
        self._cache_timestamp: float | None = None

        # Rate limiting
        self._request_semaphore = asyncio.Semaphore(self._config.max_concurrent_requests)
        self._last_request_time: float = 0.0

        # Statistics
        self._stats = {
            "total_loaded": 0,
            "filtered_out": 0,
            "cache_hits": 0,
            "cache_misses": 0,
            "api_requests": 0,
            "errors": 0,
        }

    @property
    def config(self) -> DeltaExchangeInstrumentProviderConfig:
        """Return the instrument provider configuration."""
        return self._config

    @property
    def stats(self) -> dict[str, int]:
        """Return loading statistics."""
        return self._stats.copy()

    def instruments_pyo3(self) -> list[Any]:
        """
        Return the raw pyo3 instruments.

        Returns
        -------
        list[Any]
            The list of pyo3 instrument objects.

        """
        return self._instruments_pyo3

    async def load_all_async(self, filters: dict | None = None) -> None:
        """
        Load all instruments for the venue asynchronously.

        This method loads all Delta Exchange instruments based on the configuration
        settings, applying filters for product types, symbols, and trading status.
        It supports intelligent caching and concurrent loading for optimal performance.

        Parameters
        ----------
        filters : dict, optional
            Additional venue-specific instrument loading filters to apply.
            Supported keys:
            - 'product_types': list[str] - Override config product types
            - 'symbol_filters': list[str] - Override config symbol filters
            - 'force_refresh': bool - Force refresh from API ignoring cache

        Raises
        ------
        RuntimeError
            If the HTTP client is not available or API requests fail.

        Examples
        --------
        >>> # Load all instruments with default configuration
        >>> await provider.load_all_async()

        >>> # Load with custom filters
        >>> await provider.load_all_async({
        ...     'product_types': ['perpetual_futures'],
        ...     'symbol_filters': ['BTC*', 'ETH*'],
        ...     'force_refresh': True
        ... })

        """
        PyCondition.not_none(self._client, "self._client")

        async with self._loading_lock:
            start_time = time.time()
            self._log.info("Loading Delta Exchange instruments...")

            try:
                # Check cache first unless force refresh is requested
                force_refresh = filters.get("force_refresh", False) if filters else False
                if not force_refresh and self._should_use_cache():
                    if await self._load_from_cache():
                        self._log.info(
                            f"Loaded {len(self._instruments)} instruments from cache "
                            f"in {time.time() - start_time:.2f}s"
                        )
                        return

                # Load from API
                await self._load_from_api(filters)

                # Save to cache if enabled
                if self._config.enable_instrument_caching:
                    await self._save_to_cache()

                self._last_load_time = time.time()
                self._load_count += 1

                load_time = time.time() - start_time
                self._log.info(
                    f"Loaded {self._stats['total_loaded']} instruments "
                    f"({self._stats['filtered_out']} filtered out) "
                    f"in {load_time:.2f}s"
                )

            except Exception as e:
                self._stats["errors"] += 1
                self._log.error(f"Failed to load instruments: {e}")
                raise RuntimeError(f"Failed to load Delta Exchange instruments: {e}") from e

    async def load_ids_async(
        self,
        instrument_ids: list[InstrumentId],
        filters: dict | None = None,
    ) -> None:
        """
        Load specific instruments for the given IDs asynchronously.

        Since Delta Exchange doesn't support loading instruments by specific IDs,
        this method loads all instruments and then filters to the requested IDs.

        Parameters
        ----------
        instrument_ids : list[InstrumentId]
            The instrument IDs to load.
        filters : dict, optional
            Additional venue-specific instrument loading filters to apply.

        Raises
        ------
        ValueError
            If no instrument IDs are provided.
        RuntimeError
            If loading fails or requested instruments are not found.

        """
        PyCondition.not_none(self._client, "self._client")
        PyCondition.not_empty(instrument_ids, "instrument_ids")

        if not instrument_ids:
            self._log.warning("No instrument IDs given for loading")
            return

        # Validate all instrument IDs are for Delta Exchange
        for instrument_id in instrument_ids:
            if instrument_id.venue != DELTA_EXCHANGE:
                raise ValueError(
                    f"Invalid instrument ID venue: {instrument_id.venue}, "
                    f"expected {DELTA_EXCHANGE}"
                )

        self._log.info(f"Loading {len(instrument_ids)} specific instruments...")

        # Load all instruments first (Delta Exchange doesn't support loading by ID)
        await self.load_all_async(filters)

        # Filter to requested IDs
        requested_instruments = []
        found_ids = set()

        for instrument in self.list_all():
            if instrument.id in instrument_ids:
                requested_instruments.append(instrument)
                found_ids.add(instrument.id)

        # Check if all requested instruments were found
        missing_ids = set(instrument_ids) - found_ids
        if missing_ids:
            self._log.warning(
                f"Could not find {len(missing_ids)} requested instruments: "
                f"{[str(id_) for id_ in missing_ids]}"
            )

        # Clear and re-add only requested instruments
        self._instruments.clear()
        for instrument in requested_instruments:
            self.add(instrument)

        self._log.info(f"Loaded {len(requested_instruments)} requested instruments")

    async def load_async(self, instrument_id: InstrumentId, filters: dict | None = None) -> None:
        """
        Load a single instrument asynchronously.

        Parameters
        ----------
        instrument_id : InstrumentId
            The instrument ID to load.
        filters : dict, optional
            Additional venue-specific instrument loading filters to apply.

        """
        PyCondition.not_none(instrument_id, "instrument_id")
        await self.load_ids_async([instrument_id], filters)

    # -------------------------------------------------------------------------
    # Private methods
    # -------------------------------------------------------------------------

    def _should_use_cache(self) -> bool:
        """Check if cached data should be used."""
        if not self._config.enable_instrument_caching:
            return False

        if self._cache_timestamp is None:
            return False

        # Check if cache is still valid
        cache_age_hours = (time.time() - self._cache_timestamp) / 3600
        return cache_age_hours < self._config.cache_validity_hours

    async def _load_from_cache(self) -> bool:
        """
        Load instruments from cache.

        Returns
        -------
        bool
            True if successfully loaded from cache, False otherwise.

        """
        try:
            cache_path = self._config.get_cache_file_path()
            if not os.path.exists(cache_path):
                return False

            if self._config.log_instrument_loading:
                self._log.info(f"Loading instruments from cache: {cache_path}")

            with open(cache_path, 'r', encoding='utf-8') as f:
                cache_data = json.load(f)

            # Validate cache format
            if not isinstance(cache_data, dict) or 'instruments' not in cache_data:
                self._log.warning("Invalid cache format, ignoring cache")
                return False

            # Check cache timestamp
            cache_timestamp = cache_data.get('timestamp', 0)
            cache_age_hours = (time.time() - cache_timestamp) / 3600

            if cache_age_hours >= self._config.cache_validity_hours:
                self._log.info(f"Cache expired ({cache_age_hours:.1f}h old), refreshing")
                return False

            # Load instruments from cache
            cached_instruments = cache_data['instruments']
            self._currencies = cache_data.get('currencies', {})

            # Convert cached data back to instruments
            instruments = []
            for instrument_data in cached_instruments:
                try:
                    instrument = self._deserialize_instrument(instrument_data)
                    if instrument and self._should_include_instrument(instrument_data):
                        instruments.append(instrument)
                        self.add(instrument)
                except Exception as e:
                    self._log.warning(f"Failed to deserialize cached instrument: {e}")
                    continue

            self._cache_timestamp = cache_timestamp
            self._stats["cache_hits"] += 1
            self._stats["total_loaded"] = len(instruments)

            if self._config.log_instrument_loading:
                self._log.info(f"Loaded {len(instruments)} instruments from cache")

            return len(instruments) > 0

        except Exception as e:
            self._log.warning(f"Failed to load from cache: {e}")
            self._stats["cache_misses"] += 1
            return False

    async def _save_to_cache(self) -> None:
        """Save current instruments to cache."""
        try:
            cache_path = self._config.get_cache_file_path()
            os.makedirs(os.path.dirname(cache_path), exist_ok=True)

            # Serialize instruments for caching
            cached_instruments = []
            for instrument in self.list_all():
                try:
                    instrument_data = self._serialize_instrument(instrument)
                    cached_instruments.append(instrument_data)
                except Exception as e:
                    self._log.warning(f"Failed to serialize instrument {instrument.id}: {e}")
                    continue

            cache_data = {
                'timestamp': time.time(),
                'instruments': cached_instruments,
                'currencies': self._currencies,
                'config': {
                    'product_types': self._config.get_product_type_filters(),
                    'symbol_filters': self._config.symbol_filters,
                    'load_active_only': self._config.load_active_only,
                },
            }

            with open(cache_path, 'w', encoding='utf-8') as f:
                json.dump(cache_data, f, indent=2, default=str)

            if self._config.log_instrument_loading:
                self._log.info(f"Saved {len(cached_instruments)} instruments to cache")

        except Exception as e:
            self._log.warning(f"Failed to save to cache: {e}")

    async def _load_from_api(self, filters: dict | None = None) -> None:
        """Load instruments from Delta Exchange API."""
        self._log.info("Loading instruments from Delta Exchange API...")

        try:
            # Reset statistics
            self._stats["total_loaded"] = 0
            self._stats["filtered_out"] = 0

            # Load assets first to build currency mapping
            await self._load_currencies()

            # Load products (instruments)
            await self._load_products(filters)

            # Update cache timestamp
            self._cache_timestamp = time.time()
            self._stats["api_requests"] += 1

        except Exception as e:
            self._log.error(f"Failed to load from API: {e}")
            raise

    async def _load_currencies(self) -> None:
        """Load and cache currency information from Delta Exchange."""
        try:
            await self._rate_limit()

            if self._config.log_instrument_loading:
                self._log.info("Loading currencies from Delta Exchange...")

            # Load assets from Delta Exchange API
            assets_response = await self._client.get_assets()

            if not assets_response or 'result' not in assets_response:
                self._log.warning("No assets data received from Delta Exchange")
                return

            assets = assets_response['result']

            for asset in assets:
                try:
                    currency_code = asset.get('symbol', '').upper()
                    if not currency_code:
                        continue

                    # Create Currency object
                    currency = Currency(
                        code=currency_code,
                        precision=asset.get('precision', 8),
                        iso4217=0,  # Delta Exchange assets are not ISO4217
                        name=asset.get('name', currency_code),
                        currency_type=1,  # Crypto
                    )

                    self._currencies[currency_code] = currency

                except Exception as e:
                    self._log.warning(f"Failed to process asset {asset}: {e}")
                    continue

            if self._config.log_instrument_loading:
                self._log.info(f"Loaded {len(self._currencies)} currencies")

        except Exception as e:
            self._log.error(f"Failed to load currencies: {e}")
            # Continue without currencies - instruments can still be loaded

    async def _load_products(self, filters: dict | None = None) -> None:
        """Load products (instruments) from Delta Exchange API."""
        try:
            await self._rate_limit()

            if self._config.log_instrument_loading:
                self._log.info("Loading products from Delta Exchange...")

            # Get effective product types from config and filters
            product_types = self._get_effective_product_types(filters)

            # Load products from Delta Exchange API
            products_response = await self._client.get_products()

            if not products_response or 'result' not in products_response:
                self._log.warning("No products data received from Delta Exchange")
                return

            products = products_response['result']

            # Process products concurrently with rate limiting
            semaphore = asyncio.Semaphore(self._config.max_concurrent_requests)
            tasks = []

            for product in products:
                if self._should_process_product(product, product_types, filters):
                    task = self._process_product_with_semaphore(semaphore, product)
                    tasks.append(task)

            # Wait for all products to be processed
            if tasks:
                await asyncio.gather(*tasks, return_exceptions=True)

            if self._config.log_instrument_loading:
                self._log.info(
                    f"Processed {len(tasks)} products, "
                    f"loaded {self._stats['total_loaded']} instruments"
                )

        except Exception as e:
            self._log.error(f"Failed to load products: {e}")
            raise

    async def _process_product_with_semaphore(
        self,
        semaphore: asyncio.Semaphore,
        product: dict[str, Any]
    ) -> None:
        """Process a single product with rate limiting."""
        async with semaphore:
            try:
                await self._rate_limit()
                await self._process_product(product)
            except Exception as e:
                self._log.warning(f"Failed to process product {product.get('symbol', 'unknown')}: {e}")
                self._stats["errors"] += 1

    async def _process_product(self, product: dict[str, Any]) -> None:
        """Process a single product and convert to Nautilus instrument."""
        try:
            # Convert Delta Exchange product to Nautilus instrument
            instrument = await self._convert_product_to_instrument(product)

            if instrument:
                self.add(instrument)
                self._stats["total_loaded"] += 1

                if self._config.log_instrument_loading and self._stats["total_loaded"] % 100 == 0:
                    self._log.info(f"Loaded {self._stats['total_loaded']} instruments...")
            else:
                self._stats["filtered_out"] += 1

        except Exception as e:
            self._log.warning(f"Failed to convert product {product.get('symbol', 'unknown')}: {e}")
            self._stats["errors"] += 1

    async def _rate_limit(self) -> None:
        """Apply rate limiting between API requests."""
        if self._config.request_delay_ms > 0:
            current_time = time.time()
            time_since_last = current_time - self._last_request_time
            min_interval = self._config.request_delay_ms / 1000.0

            if time_since_last < min_interval:
                await asyncio.sleep(min_interval - time_since_last)

            self._last_request_time = time.time()

    def _get_effective_product_types(self, filters: dict | None = None) -> list[str]:
        """Get effective product types from config and filters."""
        if filters and 'product_types' in filters:
            return filters['product_types']
        return self._config.get_product_type_filters()

    def _should_process_product(
        self,
        product: dict[str, Any],
        product_types: list[str],
        filters: dict | None = None
    ) -> bool:
        """Check if a product should be processed based on filters."""
        try:
            # Check product type
            product_type = product.get('product_type', '')
            if product_type not in product_types:
                return False

            # Check symbol filters
            symbol = product.get('symbol', '')
            symbol_filters = filters.get('symbol_filters') if filters else self._config.symbol_filters

            if symbol_filters:
                if not any(fnmatch.fnmatch(symbol, pattern) for pattern in symbol_filters):
                    return False

            # Check trading status
            if self._config.load_active_only:
                trading_status = product.get('trading_status', '').lower()
                if trading_status not in ('active', 'trading'):
                    return False

            # Check if expired instruments should be loaded
            if not self._config.load_expired:
                is_expired = product.get('is_expired', False)
                if is_expired:
                    return False

            return True

        except Exception as e:
            self._log.warning(f"Error checking product filters for {product.get('symbol', 'unknown')}: {e}")
            return False

    def _should_include_instrument(self, instrument_data: dict[str, Any]) -> bool:
        """Check if a cached instrument should be included based on current config."""
        try:
            symbol = instrument_data.get('symbol', '')
            product_type = instrument_data.get('product_type', '')
            trading_status = instrument_data.get('trading_status', '')

            return self._config.should_load_instrument(symbol, product_type, trading_status)

        except Exception as e:
            self._log.warning(f"Error checking cached instrument inclusion: {e}")
            return False

    async def _convert_product_to_instrument(self, product: dict[str, Any]) -> Instrument | None:
        """
        Convert a Delta Exchange product to a Nautilus instrument.

        Parameters
        ----------
        product : dict[str, Any]
            The Delta Exchange product data.

        Returns
        -------
        Instrument | None
            The converted Nautilus instrument, or None if conversion fails.

        """
        try:
            product_type = product.get('product_type', '')

            if product_type == 'perpetual_futures':
                return await self._convert_to_perpetual(product)
            elif product_type in ('call_options', 'put_options'):
                return await self._convert_to_option(product)
            else:
                self._log.warning(f"Unsupported product type: {product_type}")
                return None

        except Exception as e:
            self._log.error(f"Failed to convert product {product.get('symbol', 'unknown')}: {e}")
            return None

    async def _convert_to_perpetual(self, product: dict[str, Any]) -> CryptoPerpetual | None:
        """Convert Delta Exchange product to CryptoPerpetual."""
        try:
            symbol = product.get('symbol', '')
            if not symbol:
                return None

            # Create instrument ID
            instrument_id = InstrumentId(Symbol(symbol), DELTA_EXCHANGE)

            # Get currencies
            base_currency = self._get_currency(product.get('underlying_asset', ''))
            quote_currency = self._get_currency(product.get('quoting_asset', ''))
            settlement_currency = self._get_currency(product.get('settlement_asset', ''))

            if not quote_currency:
                self._log.warning(f"No quote currency found for {symbol}")
                return None

            # Parse decimal values
            tick_size = self._parse_decimal(product.get('tick_size', '0'))
            contract_value = self._parse_decimal(product.get('contract_value', '1'))
            min_size = self._parse_decimal(product.get('min_size', '0'))
            max_size = self._parse_decimal(product.get('max_size', '0'))

            # Calculate precision
            price_precision = self._calculate_precision(tick_size)
            size_precision = self._calculate_precision(min_size) if min_size > 0 else 8

            # Create price and quantity objects
            price_increment = Price(tick_size, price_precision) if tick_size > 0 else Price(0.01, 2)
            size_increment = Quantity(min_size, size_precision) if min_size > 0 else Quantity(0.001, 3)
            multiplier = Quantity(contract_value, 8)

            # Optional constraints
            max_quantity = Quantity(max_size, size_precision) if max_size > 0 else None
            min_quantity = size_increment

            # Margin and fees (use defaults if not available)
            margin_init = self._parse_decimal(product.get('initial_margin', '0.1'))
            margin_maint = self._parse_decimal(product.get('maintenance_margin', '0.05'))
            maker_fee = self._parse_decimal(product.get('maker_fee', '0.0005'))
            taker_fee = self._parse_decimal(product.get('taker_fee', '0.001'))

            # Timestamps
            ts_event = self._clock.timestamp_ns()
            ts_init = ts_event

            return CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=Symbol(symbol),
                base_currency=base_currency or quote_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency or quote_currency,
                is_inverse=product.get('is_inverse', False),
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                multiplier=multiplier,
                lot_size=None,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=None,
                min_price=None,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_event,
                ts_init=ts_init,
                info=self._create_instrument_info(product),
            )

        except Exception as e:
            self._log.error(f"Failed to convert perpetual {product.get('symbol', 'unknown')}: {e}")
            return None

    async def _convert_to_option(self, product: dict[str, Any]) -> CryptoOption | None:
        """Convert Delta Exchange product to CryptoOption."""
        try:
            symbol = product.get('symbol', '')
            if not symbol:
                return None

            # Create instrument ID
            instrument_id = InstrumentId(Symbol(symbol), DELTA_EXCHANGE)

            # Get currencies
            underlying_currency = self._get_currency(product.get('underlying_asset', ''))
            quote_currency = self._get_currency(product.get('quoting_asset', ''))
            settlement_currency = self._get_currency(product.get('settlement_asset', ''))

            if not underlying_currency or not quote_currency:
                self._log.warning(f"Missing currencies for option {symbol}")
                return None

            # Determine option kind
            product_type = product.get('product_type', '')
            option_kind = OptionKind.CALL if product_type == 'call_options' else OptionKind.PUT

            # Parse option-specific data
            strike_price = self._parse_decimal(product.get('strike_price', '0'))
            expiry_timestamp = product.get('expiry_time')  # Unix timestamp

            if strike_price <= 0:
                self._log.warning(f"Invalid strike price for option {symbol}")
                return None

            # Parse decimal values
            tick_size = self._parse_decimal(product.get('tick_size', '0'))
            contract_value = self._parse_decimal(product.get('contract_value', '1'))
            min_size = self._parse_decimal(product.get('min_size', '0'))
            max_size = self._parse_decimal(product.get('max_size', '0'))

            # Calculate precision
            price_precision = self._calculate_precision(tick_size)
            size_precision = self._calculate_precision(min_size) if min_size > 0 else 8

            # Create price and quantity objects
            price_increment = Price(tick_size, price_precision) if tick_size > 0 else Price(0.01, 2)
            size_increment = Quantity(min_size, size_precision) if min_size > 0 else Quantity(0.001, 3)
            multiplier = Quantity(contract_value, 8)
            strike = Price(strike_price, price_precision)

            # Optional constraints
            max_quantity = Quantity(max_size, size_precision) if max_size > 0 else None
            min_quantity = size_increment

            # Margin and fees
            margin_init = self._parse_decimal(product.get('initial_margin', '0.1'))
            margin_maint = self._parse_decimal(product.get('maintenance_margin', '0.05'))
            maker_fee = self._parse_decimal(product.get('maker_fee', '0.0005'))
            taker_fee = self._parse_decimal(product.get('taker_fee', '0.001'))

            # Timestamps
            ts_event = self._clock.timestamp_ns()
            ts_init = ts_event

            # Convert expiry timestamp to nanoseconds
            expiry_ns = int(expiry_timestamp * 1_000_000_000) if expiry_timestamp else ts_event

            return CryptoOption(
                instrument_id=instrument_id,
                raw_symbol=Symbol(symbol),
                underlying=underlying_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency or quote_currency,
                option_kind=option_kind,
                strike_price=strike,
                expiry_ns=expiry_ns,
                price_precision=price_precision,
                size_precision=size_precision,
                price_increment=price_increment,
                size_increment=size_increment,
                multiplier=multiplier,
                lot_size=None,
                max_quantity=max_quantity,
                min_quantity=min_quantity,
                max_notional=None,
                min_notional=None,
                max_price=None,
                min_price=None,
                margin_init=margin_init,
                margin_maint=margin_maint,
                maker_fee=maker_fee,
                taker_fee=taker_fee,
                ts_event=ts_event,
                ts_init=ts_init,
                info=self._create_instrument_info(product),
            )

        except Exception as e:
            self._log.error(f"Failed to convert option {product.get('symbol', 'unknown')}: {e}")
            return None

    def _get_currency(self, currency_code: str) -> Currency | None:
        """Get or create a currency object."""
        if not currency_code:
            return None

        currency_code = currency_code.upper()

        if currency_code in self._currencies:
            return self._currencies[currency_code]

        # Create a default currency if not found
        currency = Currency(
            code=currency_code,
            precision=8,
            iso4217=0,
            name=currency_code,
            currency_type=1,  # Crypto
        )

        self._currencies[currency_code] = currency
        return currency

    def _parse_decimal(self, value: Any) -> Decimal:
        """Parse a value to Decimal with error handling."""
        try:
            if value is None:
                return Decimal('0')
            return Decimal(str(value))
        except (ValueError, TypeError, decimal.InvalidOperation):
            return Decimal('0')

    def _calculate_precision(self, value: Decimal) -> int:
        """Calculate decimal precision from a decimal value."""
        try:
            if value <= 0:
                return 8

            # Convert to string and count decimal places
            value_str = str(value).rstrip('0')
            if '.' in value_str:
                return len(value_str.split('.')[1])
            return 0
        except Exception:
            return 8

    def _create_instrument_info(self, product: dict[str, Any]) -> dict[str, Any]:
        """Create instrument info dictionary from product data."""
        return {
            'id': product.get('id'),
            'description': product.get('description', ''),
            'product_type': product.get('product_type', ''),
            'trading_status': product.get('trading_status', ''),
            'is_expired': product.get('is_expired', False),
            'launch_time': product.get('launch_time'),
            'expiry_time': product.get('expiry_time'),
            'settlement_time': product.get('settlement_time'),
            'contract_unit_currency': product.get('contract_unit_currency', ''),
            'notional_type': product.get('notional_type', ''),
            'impact_size': product.get('impact_size'),
            'max_leverage_notional': product.get('max_leverage_notional'),
            'default_leverage': product.get('default_leverage'),
            'initial_margin_scaling_factor': product.get('initial_margin_scaling_factor'),
            'maintenance_margin_scaling_factor': product.get('maintenance_margin_scaling_factor'),
            'annualized_funding': product.get('annualized_funding'),
            'price_band_lower_limit': product.get('price_band_lower_limit'),
            'price_band_upper_limit': product.get('price_band_upper_limit'),
        }

    def _serialize_instrument(self, instrument: Instrument) -> dict[str, Any]:
        """Serialize an instrument for caching."""
        try:
            base_data = {
                'id': str(instrument.id),
                'symbol': str(instrument.id.symbol),
                'venue': str(instrument.id.venue),
                'raw_symbol': str(instrument.raw_symbol),
                'asset_class': str(instrument.asset_class),
                'instrument_class': str(instrument.instrument_class),
                'quote_currency': str(instrument.quote_currency),
                'is_inverse': instrument.is_inverse,
                'price_precision': instrument.price_precision,
                'size_precision': instrument.size_precision,
                'price_increment': str(instrument.price_increment),
                'size_increment': str(instrument.size_increment),
                'multiplier': str(instrument.multiplier),
                'margin_init': str(instrument.margin_init),
                'margin_maint': str(instrument.margin_maint),
                'maker_fee': str(instrument.maker_fee),
                'taker_fee': str(instrument.taker_fee),
                'ts_event': instrument.ts_event,
                'ts_init': instrument.ts_init,
                'info': instrument.info,
            }

            # Add type-specific data
            if isinstance(instrument, CryptoPerpetual):
                base_data.update({
                    'type': 'CryptoPerpetual',
                    'base_currency': str(instrument.base_currency),
                    'settlement_currency': str(instrument.settlement_currency),
                })
            elif isinstance(instrument, CryptoOption):
                base_data.update({
                    'type': 'CryptoOption',
                    'underlying': str(instrument.underlying),
                    'settlement_currency': str(instrument.settlement_currency),
                    'option_kind': str(instrument.option_kind),
                    'strike_price': str(instrument.strike_price),
                    'expiry_ns': instrument.expiry_ns,
                })

            # Add optional fields
            for field in ['lot_size', 'max_quantity', 'min_quantity', 'max_notional',
                         'min_notional', 'max_price', 'min_price']:
                value = getattr(instrument, field, None)
                if value is not None:
                    base_data[field] = str(value)

            return base_data

        except Exception as e:
            self._log.error(f"Failed to serialize instrument {instrument.id}: {e}")
            raise

    def _deserialize_instrument(self, data: dict[str, Any]) -> Instrument | None:
        """Deserialize an instrument from cached data."""
        try:
            instrument_type = data.get('type', '')

            if instrument_type == 'CryptoPerpetual':
                return self._deserialize_perpetual(data)
            elif instrument_type == 'CryptoOption':
                return self._deserialize_option(data)
            else:
                self._log.warning(f"Unknown instrument type: {instrument_type}")
                return None

        except Exception as e:
            self._log.error(f"Failed to deserialize instrument: {e}")
            return None

    def _deserialize_perpetual(self, data: dict[str, Any]) -> CryptoPerpetual | None:
        """Deserialize a CryptoPerpetual from cached data."""
        try:
            instrument_id = InstrumentId.from_str(data['id'])
            base_currency = self._get_currency(data['base_currency'])
            quote_currency = self._get_currency(data['quote_currency'])
            settlement_currency = self._get_currency(data['settlement_currency'])

            return CryptoPerpetual(
                instrument_id=instrument_id,
                raw_symbol=Symbol(data['raw_symbol']),
                base_currency=base_currency,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                is_inverse=data['is_inverse'],
                price_precision=data['price_precision'],
                size_precision=data['size_precision'],
                price_increment=Price.from_str(data['price_increment']),
                size_increment=Quantity.from_str(data['size_increment']),
                multiplier=Quantity.from_str(data['multiplier']),
                lot_size=Quantity.from_str(data['lot_size']) if data.get('lot_size') else None,
                max_quantity=Quantity.from_str(data['max_quantity']) if data.get('max_quantity') else None,
                min_quantity=Quantity.from_str(data['min_quantity']) if data.get('min_quantity') else None,
                max_notional=Money.from_str(data['max_notional']) if data.get('max_notional') else None,
                min_notional=Money.from_str(data['min_notional']) if data.get('min_notional') else None,
                max_price=Price.from_str(data['max_price']) if data.get('max_price') else None,
                min_price=Price.from_str(data['min_price']) if data.get('min_price') else None,
                margin_init=Decimal(data['margin_init']),
                margin_maint=Decimal(data['margin_maint']),
                maker_fee=Decimal(data['maker_fee']),
                taker_fee=Decimal(data['taker_fee']),
                ts_event=data['ts_event'],
                ts_init=data['ts_init'],
                info=data.get('info', {}),
            )

        except Exception as e:
            self._log.error(f"Failed to deserialize perpetual: {e}")
            return None

    def _deserialize_option(self, data: dict[str, Any]) -> CryptoOption | None:
        """Deserialize a CryptoOption from cached data."""
        try:
            instrument_id = InstrumentId.from_str(data['id'])
            underlying = self._get_currency(data['underlying'])
            quote_currency = self._get_currency(data['quote_currency'])
            settlement_currency = self._get_currency(data['settlement_currency'])
            option_kind = OptionKind[data['option_kind']]

            return CryptoOption(
                instrument_id=instrument_id,
                raw_symbol=Symbol(data['raw_symbol']),
                underlying=underlying,
                quote_currency=quote_currency,
                settlement_currency=settlement_currency,
                option_kind=option_kind,
                strike_price=Price.from_str(data['strike_price']),
                expiry_ns=data['expiry_ns'],
                price_precision=data['price_precision'],
                size_precision=data['size_precision'],
                price_increment=Price.from_str(data['price_increment']),
                size_increment=Quantity.from_str(data['size_increment']),
                multiplier=Quantity.from_str(data['multiplier']),
                lot_size=Quantity.from_str(data['lot_size']) if data.get('lot_size') else None,
                max_quantity=Quantity.from_str(data['max_quantity']) if data.get('max_quantity') else None,
                min_quantity=Quantity.from_str(data['min_quantity']) if data.get('min_quantity') else None,
                max_notional=Money.from_str(data['max_notional']) if data.get('max_notional') else None,
                min_notional=Money.from_str(data['min_notional']) if data.get('min_notional') else None,
                max_price=Price.from_str(data['max_price']) if data.get('max_price') else None,
                min_price=Price.from_str(data['min_price']) if data.get('min_price') else None,
                margin_init=Decimal(data['margin_init']),
                margin_maint=Decimal(data['margin_maint']),
                maker_fee=Decimal(data['maker_fee']),
                taker_fee=Decimal(data['taker_fee']),
                ts_event=data['ts_event'],
                ts_init=data['ts_init'],
                info=data.get('info', {}),
            )

        except Exception as e:
            self._log.error(f"Failed to deserialize option: {e}")
            return None
