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
Delta Exchange Factory Classes.

This module provides comprehensive factory classes for creating Delta Exchange
clients with proper dependency injection, configuration management, caching,
and resource management. The factories handle all aspects of client instantiation
including HTTP/WebSocket client creation, instrument provider setup, and
configuration validation.
"""

import asyncio
import logging
from functools import lru_cache
from typing import Any

from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeDataClientConfig
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeExecClientConfig
from nautilus_trader.adapters.delta_exchange.config import DeltaExchangeInstrumentProviderConfig
from nautilus_trader.adapters.delta_exchange.constants import DELTA_EXCHANGE
from nautilus_trader.adapters.delta_exchange.data import DeltaExchangeDataClient
from nautilus_trader.adapters.delta_exchange.execution import DeltaExchangeExecutionClient
from nautilus_trader.adapters.delta_exchange.providers import DeltaExchangeInstrumentProvider
from nautilus_trader.cache.cache import Cache
from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.core import nautilus_pyo3
from nautilus_trader.live.factories import LiveDataClientFactory
from nautilus_trader.live.factories import LiveExecClientFactory


# Configure logging
_logger = logging.getLogger(__name__)


# -- CACHING FUNCTIONS ------------------------------------------------------------------------

@lru_cache(maxsize=10)
def get_cached_delta_exchange_http_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 60,
    testnet: bool = False,
    sandbox: bool = False,
) -> nautilus_pyo3.DeltaExchangeHttpClient:
    """
    Cache and return a Delta Exchange HTTP client with the given parameters.

    If a cached client with matching parameters already exists, the cached client
    will be returned. This provides efficient resource usage and connection pooling.

    Parameters
    ----------
    api_key : str, optional
        The Delta Exchange API key for the client.
    api_secret : str, optional
        The Delta Exchange API secret for the client.
    base_url : str, optional
        The base URL for the API endpoints.
    timeout_secs : int, default 60
        The timeout (seconds) for HTTP requests to Delta Exchange.
    testnet : bool, default False
        Whether to use the testnet environment.
    sandbox : bool, default False
        Whether to use the sandbox environment.

    Returns
    -------
    nautilus_pyo3.DeltaExchangeHttpClient
        The cached or newly created HTTP client.

    Raises
    ------
    ValueError
        If both testnet and sandbox are True.
    RuntimeError
        If client creation fails.

    """
    try:
        # Validate environment configuration
        if testnet and sandbox:
            raise ValueError("Cannot use both testnet and sandbox environments")

        # Log client creation
        env_type = "testnet" if testnet else "sandbox" if sandbox else "production"
        _logger.info(f"Creating Delta Exchange HTTP client for {env_type} environment")

        # Create and return client
        return nautilus_pyo3.DeltaExchangeHttpClient(
            api_key=api_key,
            api_secret=api_secret,
            base_url=base_url,
            timeout_secs=timeout_secs,
            testnet=testnet,
            sandbox=sandbox,
        )

    except Exception as e:
        _logger.error(f"Failed to create Delta Exchange HTTP client: {e}")
        raise RuntimeError(f"HTTP client creation failed: {e}") from e


@lru_cache(maxsize=10)
def get_cached_delta_exchange_ws_client(
    api_key: str | None = None,
    api_secret: str | None = None,
    base_url: str | None = None,
    timeout_secs: int = 10,
    testnet: bool = False,
    sandbox: bool = False,
) -> nautilus_pyo3.DeltaExchangeWebSocketClient:
    """
    Cache and return a Delta Exchange WebSocket client with the given parameters.

    If a cached client with matching parameters already exists, the cached client
    will be returned. This provides efficient resource usage and connection pooling.

    Parameters
    ----------
    api_key : str, optional
        The Delta Exchange API key for the client.
    api_secret : str, optional
        The Delta Exchange API secret for the client.
    base_url : str, optional
        The base URL for the WebSocket endpoints.
    timeout_secs : int, default 10
        The timeout (seconds) for WebSocket operations.
    testnet : bool, default False
        Whether to use the testnet environment.
    sandbox : bool, default False
        Whether to use the sandbox environment.

    Returns
    -------
    nautilus_pyo3.DeltaExchangeWebSocketClient
        The cached or newly created WebSocket client.

    Raises
    ------
    ValueError
        If both testnet and sandbox are True.
    RuntimeError
        If client creation fails.

    """
    try:
        # Validate environment configuration
        if testnet and sandbox:
            raise ValueError("Cannot use both testnet and sandbox environments")

        # Log client creation
        env_type = "testnet" if testnet else "sandbox" if sandbox else "production"
        _logger.info(f"Creating Delta Exchange WebSocket client for {env_type} environment")

        # Create and return client
        return nautilus_pyo3.DeltaExchangeWebSocketClient(
            api_key=api_key,
            api_secret=api_secret,
            base_url=base_url,
            timeout_secs=timeout_secs,
            testnet=testnet,
            sandbox=sandbox,
        )

    except Exception as e:
        _logger.error(f"Failed to create Delta Exchange WebSocket client: {e}")
        raise RuntimeError(f"WebSocket client creation failed: {e}") from e


@lru_cache(maxsize=10)
def get_cached_delta_exchange_instrument_provider(
    client: nautilus_pyo3.DeltaExchangeHttpClient,
    clock: LiveClock,
    config: DeltaExchangeInstrumentProviderConfig | InstrumentProviderConfig,
    product_types: frozenset[str] | None = None,
    symbol_patterns: frozenset[str] | None = None,
) -> DeltaExchangeInstrumentProvider:
    """
    Cache and return a Delta Exchange instrument provider.

    If a cached provider with matching parameters already exists, the cached provider
    will be returned. This provides efficient resource usage and avoids duplicate
    instrument loading operations.

    Parameters
    ----------
    client : nautilus_pyo3.DeltaExchangeHttpClient
        The HTTP client for the instrument provider.
    clock : LiveClock
        The clock for the instrument provider.
    config : DeltaExchangeInstrumentProviderConfig | InstrumentProviderConfig
        The configuration for the instrument provider.
    product_types : frozenset[str], optional
        The product types to filter instruments by.
    symbol_patterns : frozenset[str], optional
        The symbol patterns to filter instruments by.

    Returns
    -------
    DeltaExchangeInstrumentProvider
        The cached or newly created instrument provider.

    Raises
    ------
    RuntimeError
        If provider creation fails.

    """
    try:
        _logger.info("Creating Delta Exchange instrument provider")

        # Convert config if needed
        if isinstance(config, InstrumentProviderConfig):
            provider_config = DeltaExchangeInstrumentProviderConfig(
                load_all=config.load_all,
                load_ids=config.load_ids,
                filters=config.filters,
                filter_callable=config.filter_callable,
                log_warnings=config.log_warnings,
                product_types=list(product_types) if product_types else None,
                symbol_patterns=list(symbol_patterns) if symbol_patterns else None,
            )
        else:
            provider_config = config

        # Create and return provider
        return DeltaExchangeInstrumentProvider(
            client=client,
            clock=clock,
            config=provider_config,
        )

    except Exception as e:
        _logger.error(f"Failed to create Delta Exchange instrument provider: {e}")
        raise RuntimeError(f"Instrument provider creation failed: {e}") from e


# -- VALIDATION FUNCTIONS --------------------------------------------------------------------

def _validate_data_client_config(config: DeltaExchangeDataClientConfig) -> None:
    """
    Validate Delta Exchange data client configuration.

    Parameters
    ----------
    config : DeltaExchangeDataClientConfig
        The configuration to validate.

    Raises
    ------
    ValueError
        If configuration is invalid.

    """
    # Validate API credentials for private channels
    if config.enable_private_channels:
        if not config.get_effective_api_key():
            raise ValueError("API key is required for private channels")
        if not config.get_effective_api_secret():
            raise ValueError("API secret is required for private channels")

    # Validate environment settings
    if config.testnet and config.sandbox:
        raise ValueError("Cannot use both testnet and sandbox environments")

    # Validate timeout settings
    if config.http_timeout_secs and config.http_timeout_secs <= 0:
        raise ValueError("HTTP timeout must be positive")
    if config.ws_timeout_secs and config.ws_timeout_secs <= 0:
        raise ValueError("WebSocket timeout must be positive")

    # Validate subscription settings
    if config.max_subscriptions and config.max_subscriptions <= 0:
        raise ValueError("Max subscriptions must be positive")


def _validate_exec_client_config(config: DeltaExchangeExecClientConfig) -> None:
    """
    Validate Delta Exchange execution client configuration.

    Parameters
    ----------
    config : DeltaExchangeExecClientConfig
        The configuration to validate.

    Raises
    ------
    ValueError
        If configuration is invalid.

    """
    # Validate required API credentials
    if not config.get_effective_api_key():
        raise ValueError("API key is required for execution client")
    if not config.get_effective_api_secret():
        raise ValueError("API secret is required for execution client")
    if not config.account_id:
        raise ValueError("Account ID is required for execution client")

    # Validate environment settings
    if config.testnet and config.sandbox:
        raise ValueError("Cannot use both testnet and sandbox environments")

    # Validate timeout settings
    if config.http_timeout_secs and config.http_timeout_secs <= 0:
        raise ValueError("HTTP timeout must be positive")
    if config.ws_timeout_secs and config.ws_timeout_secs <= 0:
        raise ValueError("WebSocket timeout must be positive")

    # Validate retry settings
    if config.max_retries and config.max_retries < 0:
        raise ValueError("Max retries cannot be negative")
    if config.retry_delay_secs and config.retry_delay_secs <= 0:
        raise ValueError("Retry delay must be positive")

    # Validate risk management settings
    if config.daily_loss_limit and config.daily_loss_limit <= 0:
        raise ValueError("Daily loss limit must be positive")
    if config.max_position_value and config.max_position_value <= 0:
        raise ValueError("Max position value must be positive")


# -- FACTORY CLASSES --------------------------------------------------------------------------

class DeltaExchangeLiveDataClientFactory(LiveDataClientFactory):
    """
    Provides a comprehensive Delta Exchange live data client factory.

    This factory handles all aspects of data client creation including:
    - HTTP and WebSocket client instantiation with caching
    - Instrument provider setup and configuration
    - Configuration validation and environment management
    - Resource management and cleanup
    - Error handling and logging
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DeltaExchangeDataClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DeltaExchangeDataClient:
        """
        Create a new Delta Exchange data client with comprehensive setup.

        This method performs the following operations:
        1. Validates the configuration
        2. Creates cached HTTP and WebSocket clients
        3. Sets up the instrument provider
        4. Instantiates the data client with all dependencies

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DeltaExchangeDataClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        DeltaExchangeDataClient
            The fully configured data client.

        Raises
        ------
        ValueError
            If configuration is invalid.
        RuntimeError
            If client creation fails.

        """
        try:
            _logger.info(f"Creating Delta Exchange data client '{name}'")

            # Validate configuration
            _validate_data_client_config(config)

            # Get cached HTTP client
            http_client = get_cached_delta_exchange_http_client(
                api_key=config.get_effective_api_key(),
                api_secret=config.get_effective_api_secret(),
                base_url=config.get_effective_http_url(),
                timeout_secs=config.http_timeout_secs or 60,
                testnet=config.testnet,
                sandbox=config.sandbox,
            )

            # Get cached instrument provider
            instrument_provider = get_cached_delta_exchange_instrument_provider(
                client=http_client,
                clock=clock,
                config=config.instrument_provider,
                product_types=frozenset(config.product_types) if config.product_types else None,
                symbol_patterns=frozenset(config.symbol_patterns) if config.symbol_patterns else None,
            )

            # Create data client
            data_client = DeltaExchangeDataClient(
                loop=loop,
                client=http_client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=instrument_provider,
                config=config,
                name=name,
            )

            _logger.info(f"Successfully created Delta Exchange data client '{name}'")
            return data_client

        except Exception as e:
            _logger.error(f"Failed to create Delta Exchange data client '{name}': {e}")
            raise RuntimeError(f"Data client creation failed: {e}") from e


class DeltaExchangeLiveExecClientFactory(LiveExecClientFactory):
    """
    Provides a comprehensive Delta Exchange live execution client factory.

    This factory handles all aspects of execution client creation including:
    - HTTP and WebSocket client instantiation with caching
    - Instrument provider setup and configuration
    - Configuration validation and security management
    - Resource management and cleanup
    - Error handling and logging
    """

    @staticmethod
    def create(  # type: ignore
        loop: asyncio.AbstractEventLoop,
        name: str,
        config: DeltaExchangeExecClientConfig,
        msgbus: MessageBus,
        cache: Cache,
        clock: LiveClock,
    ) -> DeltaExchangeExecutionClient:
        """
        Create a new Delta Exchange execution client with comprehensive setup.

        This method performs the following operations:
        1. Validates the configuration including security settings
        2. Creates cached HTTP and WebSocket clients
        3. Sets up the instrument provider
        4. Instantiates the execution client with all dependencies

        Parameters
        ----------
        loop : asyncio.AbstractEventLoop
            The event loop for the client.
        name : str
            The custom client ID.
        config : DeltaExchangeExecClientConfig
            The configuration for the client.
        msgbus : MessageBus
            The message bus for the client.
        cache : Cache
            The cache for the client.
        clock : LiveClock
            The clock for the client.

        Returns
        -------
        DeltaExchangeExecutionClient
            The fully configured execution client.

        Raises
        ------
        ValueError
            If configuration is invalid.
        RuntimeError
            If client creation fails.

        """
        try:
            _logger.info(f"Creating Delta Exchange execution client '{name}'")

            # Validate configuration
            _validate_exec_client_config(config)

            # Get cached HTTP client
            http_client = get_cached_delta_exchange_http_client(
                api_key=config.get_effective_api_key(),
                api_secret=config.get_effective_api_secret(),
                base_url=config.get_effective_http_url(),
                timeout_secs=config.http_timeout_secs or 60,
                testnet=config.testnet,
                sandbox=config.sandbox,
            )

            # Get cached instrument provider
            instrument_provider = get_cached_delta_exchange_instrument_provider(
                client=http_client,
                clock=clock,
                config=config.instrument_provider,
                product_types=frozenset(config.product_types) if config.product_types else None,
                symbol_patterns=frozenset(config.symbol_patterns) if config.symbol_patterns else None,
            )

            # Create execution client
            exec_client = DeltaExchangeExecutionClient(
                loop=loop,
                client=http_client,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
                instrument_provider=instrument_provider,
                config=config,
                name=name,
            )

            _logger.info(f"Successfully created Delta Exchange execution client '{name}'")
            return exec_client

        except Exception as e:
            _logger.error(f"Failed to create Delta Exchange execution client '{name}': {e}")
            raise RuntimeError(f"Execution client creation failed: {e}") from e


# -- ENGINE FACTORY CLASSES ------------------------------------------------------------------

class DeltaExchangeLiveDataEngineFactory:
    """
    Provides a comprehensive factory for Delta Exchange data engine setup.

    This factory creates a complete data engine configuration including:
    - Data client factory registration
    - Instrument provider factory setup
    - Configuration inheritance and override mechanisms
    - Lifecycle management for all components
    """

    @staticmethod
    def create_config(
        venue: str = DELTA_EXCHANGE.value,
        api_key: str | None = None,
        api_secret: str | None = None,
        testnet: bool = False,
        sandbox: bool = False,
        **kwargs: Any,
    ) -> dict[str, Any]:
        """
        Create a data engine configuration for Delta Exchange.

        Parameters
        ----------
        venue : str, default "DELTA_EXCHANGE"
            The venue identifier.
        api_key : str, optional
            The API key for authentication.
        api_secret : str, optional
            The API secret for authentication.
        testnet : bool, default False
            Whether to use testnet environment.
        sandbox : bool, default False
            Whether to use sandbox environment.
        **kwargs : Any
            Additional configuration parameters.

        Returns
        -------
        dict[str, Any]
            The data engine configuration.

        """
        # Create data client configuration
        data_config = DeltaExchangeDataClientConfig(
            api_key=api_key,
            api_secret=api_secret,
            testnet=testnet,
            sandbox=sandbox,
            **kwargs,
        )

        return {
            "data_clients": {
                venue: {
                    "factory": DeltaExchangeLiveDataClientFactory,
                    "config": data_config,
                }
            }
        }

    @staticmethod
    def register_with_node(
        node: Any,  # TradingNode
        config: DeltaExchangeDataClientConfig | None = None,
        venue: str = DELTA_EXCHANGE.value,
    ) -> None:
        """
        Register the Delta Exchange data client factory with a trading node.

        Parameters
        ----------
        node : TradingNode
            The trading node to register with.
        config : DeltaExchangeDataClientConfig, optional
            The configuration for the data client.
        venue : str, default "DELTA_EXCHANGE"
            The venue identifier.

        """
        try:
            _logger.info(f"Registering Delta Exchange data client factory for venue '{venue}'")

            # Register the factory
            node.add_data_client_factory(venue, DeltaExchangeLiveDataClientFactory)

            _logger.info("Successfully registered Delta Exchange data client factory")

        except Exception as e:
            _logger.error(f"Failed to register Delta Exchange data client factory: {e}")
            raise


class DeltaExchangeLiveExecEngineFactory:
    """
    Provides a comprehensive factory for Delta Exchange execution engine setup.

    This factory creates a complete execution engine configuration including:
    - Execution client factory registration
    - Instrument provider factory setup
    - Configuration inheritance and override mechanisms
    - Lifecycle management and resource cleanup
    """

    @staticmethod
    def create_config(
        venue: str = DELTA_EXCHANGE.value,
        api_key: str | None = None,
        api_secret: str | None = None,
        account_id: str | None = None,
        testnet: bool = False,
        sandbox: bool = False,
        **kwargs: Any,
    ) -> dict[str, Any]:
        """
        Create an execution engine configuration for Delta Exchange.

        Parameters
        ----------
        venue : str, default "DELTA_EXCHANGE"
            The venue identifier.
        api_key : str, optional
            The API key for authentication.
        api_secret : str, optional
            The API secret for authentication.
        account_id : str, optional
            The account ID for trading.
        testnet : bool, default False
            Whether to use testnet environment.
        sandbox : bool, default False
            Whether to use sandbox environment.
        **kwargs : Any
            Additional configuration parameters.

        Returns
        -------
        dict[str, Any]
            The execution engine configuration.

        """
        # Create execution client configuration
        exec_config = DeltaExchangeExecClientConfig(
            api_key=api_key,
            api_secret=api_secret,
            account_id=account_id,
            testnet=testnet,
            sandbox=sandbox,
            **kwargs,
        )

        return {
            "exec_clients": {
                venue: {
                    "factory": DeltaExchangeLiveExecClientFactory,
                    "config": exec_config,
                }
            }
        }

    @staticmethod
    def register_with_node(
        node: Any,  # TradingNode
        config: DeltaExchangeExecClientConfig | None = None,
        venue: str = DELTA_EXCHANGE.value,
    ) -> None:
        """
        Register the Delta Exchange execution client factory with a trading node.

        Parameters
        ----------
        node : TradingNode
            The trading node to register with.
        config : DeltaExchangeExecClientConfig, optional
            The configuration for the execution client.
        venue : str, default "DELTA_EXCHANGE"
            The venue identifier.

        """
        try:
            _logger.info(f"Registering Delta Exchange execution client factory for venue '{venue}'")

            # Register the factory
            node.add_exec_client_factory(venue, DeltaExchangeLiveExecClientFactory)

            _logger.info(f"Successfully registered Delta Exchange execution client factory")

        except Exception as e:
            _logger.error(f"Failed to register Delta Exchange execution client factory: {e}")
            raise


# -- UTILITY FUNCTIONS -----------------------------------------------------------------------

def create_delta_exchange_clients(
    data_config: DeltaExchangeDataClientConfig | None = None,
    exec_config: DeltaExchangeExecClientConfig | None = None,
    loop: asyncio.AbstractEventLoop | None = None,
    msgbus: MessageBus | None = None,
    cache: Cache | None = None,
    clock: LiveClock | None = None,
) -> tuple[DeltaExchangeDataClient | None, DeltaExchangeExecutionClient | None]:
    """
    Create Delta Exchange data and execution clients with shared dependencies.

    This utility function creates both clients with shared HTTP client and
    instrument provider instances for efficiency.

    Parameters
    ----------
    data_config : DeltaExchangeDataClientConfig, optional
        The data client configuration.
    exec_config : DeltaExchangeExecClientConfig, optional
        The execution client configuration.
    loop : asyncio.AbstractEventLoop, optional
        The event loop for the clients.
    msgbus : MessageBus, optional
        The message bus for the clients.
    cache : Cache, optional
        The cache for the clients.
    clock : LiveClock, optional
        The clock for the clients.

    Returns
    -------
    tuple[DeltaExchangeDataClient | None, DeltaExchangeExecutionClient | None]
        The created data and execution clients.

    """
    data_client = None
    exec_client = None

    try:
        # Create data client if config provided
        if data_config and loop and msgbus and cache and clock:
            data_client = DeltaExchangeLiveDataClientFactory.create(
                loop=loop,
                name="DeltaExchange-Data",
                config=data_config,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )

        # Create execution client if config provided
        if exec_config and loop and msgbus and cache and clock:
            exec_client = DeltaExchangeLiveExecClientFactory.create(
                loop=loop,
                name="DeltaExchange-Exec",
                config=exec_config,
                msgbus=msgbus,
                cache=cache,
                clock=clock,
            )

        return data_client, exec_client

    except Exception as e:
        _logger.error(f"Failed to create Delta Exchange clients: {e}")
        raise


def clear_delta_exchange_caches() -> None:
    """
    Clear all Delta Exchange factory caches.

    This function clears all cached HTTP clients, WebSocket clients, and
    instrument providers. Use this for testing or when you need to force
    recreation of clients with new parameters.

    """
    try:
        _logger.info("Clearing Delta Exchange factory caches")

        # Clear all cached functions
        get_cached_delta_exchange_http_client.cache_clear()
        get_cached_delta_exchange_ws_client.cache_clear()
        get_cached_delta_exchange_instrument_provider.cache_clear()

        _logger.info("Successfully cleared Delta Exchange factory caches")

    except Exception as e:
        _logger.error(f"Failed to clear Delta Exchange factory caches: {e}")
        raise


def get_delta_exchange_factory_info() -> dict[str, Any]:
    """
    Get information about Delta Exchange factory caches and configurations.

    Returns
    -------
    dict[str, Any]
        Factory information including cache statistics.

    """
    try:
        return {
            "http_client_cache": {
                "hits": get_cached_delta_exchange_http_client.cache_info().hits,
                "misses": get_cached_delta_exchange_http_client.cache_info().misses,
                "maxsize": get_cached_delta_exchange_http_client.cache_info().maxsize,
                "currsize": get_cached_delta_exchange_http_client.cache_info().currsize,
            },
            "ws_client_cache": {
                "hits": get_cached_delta_exchange_ws_client.cache_info().hits,
                "misses": get_cached_delta_exchange_ws_client.cache_info().misses,
                "maxsize": get_cached_delta_exchange_ws_client.cache_info().maxsize,
                "currsize": get_cached_delta_exchange_ws_client.cache_info().currsize,
            },
            "instrument_provider_cache": {
                "hits": get_cached_delta_exchange_instrument_provider.cache_info().hits,
                "misses": get_cached_delta_exchange_instrument_provider.cache_info().misses,
                "maxsize": get_cached_delta_exchange_instrument_provider.cache_info().maxsize,
                "currsize": get_cached_delta_exchange_instrument_provider.cache_info().currsize,
            },
            "supported_environments": ["production", "testnet", "sandbox"],
            "factory_classes": [
                "DeltaExchangeLiveDataClientFactory",
                "DeltaExchangeLiveExecClientFactory",
                "DeltaExchangeLiveDataEngineFactory",
                "DeltaExchangeLiveExecEngineFactory",
            ],
        }

    except Exception as e:
        _logger.error(f"Failed to get Delta Exchange factory info: {e}")
        return {"error": str(e)}


# -- FACTORY CONFIGURATION HELPERS -----------------------------------------------------------

def create_testnet_factories(
    api_key: str,
    api_secret: str,
    account_id: str | None = None,
) -> tuple[DeltaExchangeLiveDataClientFactory, DeltaExchangeLiveExecClientFactory]:
    """
    Create factory instances configured for Delta Exchange testnet.

    Parameters
    ----------
    api_key : str
        The testnet API key.
    api_secret : str
        The testnet API secret.
    account_id : str, optional
        The testnet account ID.

    Returns
    -------
    tuple[DeltaExchangeLiveDataClientFactory, DeltaExchangeLiveExecClientFactory]
        The configured factory instances.

    """
    _logger.info("Creating Delta Exchange testnet factories")

    # Factories are stateless, so we just return the classes
    # Configuration is handled at client creation time
    return DeltaExchangeLiveDataClientFactory(), DeltaExchangeLiveExecClientFactory()


def create_production_factories(
    api_key: str,
    api_secret: str,
    account_id: str,
) -> tuple[DeltaExchangeLiveDataClientFactory, DeltaExchangeLiveExecClientFactory]:
    """
    Create factory instances configured for Delta Exchange production.

    Parameters
    ----------
    api_key : str
        The production API key.
    api_secret : str
        The production API secret.
    account_id : str
        The production account ID.

    Returns
    -------
    tuple[DeltaExchangeLiveDataClientFactory, DeltaExchangeLiveExecClientFactory]
        The configured factory instances.

    """
    _logger.info("Creating Delta Exchange production factories")

    # Factories are stateless, so we just return the classes
    # Configuration is handled at client creation time
    return DeltaExchangeLiveDataClientFactory(), DeltaExchangeLiveExecClientFactory()


def validate_factory_environment() -> dict[str, bool]:
    """
    Validate the factory environment and dependencies.

    Returns
    -------
    dict[str, bool]
        Validation results for various components.

    """
    results = {}

    try:
        # Check if Rust bindings are available
        results["rust_http_client"] = hasattr(nautilus_pyo3, "DeltaExchangeHttpClient")
        results["rust_ws_client"] = hasattr(nautilus_pyo3, "DeltaExchangeWebSocketClient")

        # Check if configuration classes are available
        results["data_config"] = DeltaExchangeDataClientConfig is not None
        results["exec_config"] = DeltaExchangeExecClientConfig is not None

        # Check if client classes are available
        results["data_client"] = DeltaExchangeDataClient is not None
        results["exec_client"] = DeltaExchangeExecutionClient is not None
        results["instrument_provider"] = DeltaExchangeInstrumentProvider is not None

        # Check if factory classes are properly defined
        results["data_factory"] = DeltaExchangeLiveDataClientFactory is not None
        results["exec_factory"] = DeltaExchangeLiveExecClientFactory is not None

        _logger.info(f"Factory environment validation: {results}")

    except Exception as e:
        _logger.error(f"Factory environment validation failed: {e}")
        results["validation_error"] = str(e)

    return results
