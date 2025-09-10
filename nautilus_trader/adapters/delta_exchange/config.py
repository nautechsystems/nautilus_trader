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
Configuration classes for the Delta Exchange adapter.

This module provides comprehensive configuration classes for Delta Exchange integration,
including data client, execution client, and instrument provider configurations with
proper validation, security, and environment management.
"""

from __future__ import annotations

import os
import re
from typing import Any

from nautilus_trader.adapters.delta_exchange.constants import (
    DELTA_EXCHANGE,
    DELTA_EXCHANGE_BASE_URL,
    DELTA_EXCHANGE_PRODUCT_TYPES,
    DELTA_EXCHANGE_TESTNET_BASE_URL,
    DELTA_EXCHANGE_TESTNET_WS_URL,
    DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    DELTA_EXCHANGE_WS_URL,
)
from nautilus_trader.config import InstrumentProviderConfig
from nautilus_trader.config import InvalidConfiguration
from nautilus_trader.config import LiveDataClientConfig
from nautilus_trader.config import LiveExecClientConfig
from nautilus_trader.config import NautilusConfig
from nautilus_trader.config import NonNegativeFloat
from nautilus_trader.config import NonNegativeInt
from nautilus_trader.config import PositiveFloat
from nautilus_trader.config import PositiveInt
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import Venue


# Environment variable names
ENV_API_KEY = "DELTA_EXCHANGE_API_KEY"
ENV_API_SECRET = "DELTA_EXCHANGE_API_SECRET"
ENV_TESTNET_API_KEY = "DELTA_EXCHANGE_TESTNET_API_KEY"
ENV_TESTNET_API_SECRET = "DELTA_EXCHANGE_TESTNET_API_SECRET"
ENV_SANDBOX_API_KEY = "DELTA_EXCHANGE_SANDBOX_API_KEY"
ENV_SANDBOX_API_SECRET = "DELTA_EXCHANGE_SANDBOX_API_SECRET"

# Default configuration values
DEFAULT_HTTP_TIMEOUT = 60
DEFAULT_WS_TIMEOUT = 30
DEFAULT_RECONNECTION_DELAY = 5
DEFAULT_MAX_RECONNECTION_ATTEMPTS = 10
DEFAULT_HEARTBEAT_INTERVAL = 30
DEFAULT_MAX_QUEUE_SIZE = 10000
DEFAULT_UPDATE_INSTRUMENTS_INTERVAL = 60
DEFAULT_CACHE_VALIDITY_HOURS = 24

# Validation constants
MIN_TIMEOUT_SECS = 1
MAX_TIMEOUT_SECS = 300
MIN_RECONNECTION_DELAY = 1
MAX_RECONNECTION_DELAY = 60
MIN_HEARTBEAT_INTERVAL = 10
MAX_HEARTBEAT_INTERVAL = 300
MIN_UPDATE_INTERVAL = 1
MAX_UPDATE_INTERVAL = 1440  # 24 hours in minutes


def _validate_url(url: str | None, name: str) -> None:
    """Validate URL format."""
    if url is None:
        return

    url_pattern = re.compile(
        r"^https?://"  # http:// or https://
        r"(?:(?:[A-Z0-9](?:[A-Z0-9-]{0,61}[A-Z0-9])?\.)+[A-Z]{2,6}\.?|"  # domain...
        r"localhost|"  # localhost...
        r"\d{1,3}\.\d{1,3}\.\d{1,3}\.\d{1,3})"  # ...or ip
        r"(?::\d+)?"  # optional port
        r"(?:/?|[/?]\S+)$", re.IGNORECASE
    )

    if not url_pattern.match(url):
        raise InvalidConfiguration(f"Invalid {name} URL format: {url}")


def _validate_api_credentials(api_key: str | None, api_secret: str | None) -> None:
    """Validate API credentials format."""
    if api_key is not None and (not api_key or len(api_key) < 10):
        raise InvalidConfiguration("API key must be at least 10 characters long")

    if api_secret is not None and (not api_secret or len(api_secret) < 20):
        raise InvalidConfiguration("API secret must be at least 20 characters long")


def _get_env_credentials(testnet: bool = False, sandbox: bool = False) -> tuple[str | None, str | None]:
    """Get API credentials from environment variables."""
    if sandbox:
        api_key = os.environ.get(ENV_SANDBOX_API_KEY)
        api_secret = os.environ.get(ENV_SANDBOX_API_SECRET)
    elif testnet:
        api_key = os.environ.get(ENV_TESTNET_API_KEY)
        api_secret = os.environ.get(ENV_TESTNET_API_SECRET)
    else:
        api_key = os.environ.get(ENV_API_KEY)
        api_secret = os.environ.get(ENV_API_SECRET)

    return api_key, api_secret


class DeltaExchangeDataClientConfig(LiveDataClientConfig, frozen=True):
    """
    Configuration for ``DeltaExchangeDataClient`` instances.

    This configuration class provides comprehensive settings for the Delta Exchange
    data client including API credentials, WebSocket settings, subscription management,
    and environment-specific configurations.

    Parameters
    ----------
    venue : Venue, default DELTA_EXCHANGE
        The venue for the client.
    api_key : str, optional
        The Delta Exchange API key.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_KEY`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_KEY`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_KEY`
    api_secret : str, optional
        The Delta Exchange API secret.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_SECRET`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_SECRET`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_SECRET`
    base_url_http : str, optional
        The HTTP client custom endpoint override.
        Must be a valid HTTP/HTTPS URL.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
        Must be a valid WebSocket URL (ws:// or wss://).
    http_timeout_secs : PositiveInt, default 60
        The timeout (seconds) for HTTP requests.
        Must be between 1 and 300 seconds.
    ws_timeout_secs : PositiveInt, default 30
        The timeout (seconds) for WebSocket connections.
        Must be between 1 and 300 seconds.
    testnet : bool, default False
        If the client should connect to the testnet environment.
        Cannot be True if sandbox is True.
    sandbox : bool, default False
        If the client should connect to the sandbox environment.
        Cannot be True if testnet is True.
    auto_reconnect : bool, default True
        If the WebSocket client should automatically reconnect on disconnection.
    reconnection_delay_secs : PositiveInt, default 5
        The initial delay (seconds) between reconnection attempts.
        Must be between 1 and 60 seconds.
    max_reconnection_attempts : PositiveInt, default 10
        The maximum number of reconnection attempts.
    heartbeat_interval_secs : PositiveInt, default 30
        The interval (seconds) for WebSocket heartbeat messages.
        Must be between 10 and 300 seconds.
    max_queue_size : PositiveInt, default 10000
        The maximum size of the message queue during disconnections.
    default_channels : list[str], optional
        The default WebSocket channels to subscribe to on connection.
        Must be valid Delta Exchange channel names.
    symbol_filters : list[str], optional
        Symbol patterns to filter subscriptions (e.g., ["BTC*", "ETH*"]).
    rate_limit_requests_per_second : PositiveInt, default 75
        The maximum number of requests per second to avoid rate limiting.
        Delta Exchange allows up to 100 requests per second.
    log_raw_messages : bool, default False
        If raw WebSocket messages should be logged for debugging.

    Raises
    ------
    InvalidConfiguration
        If configuration parameters are invalid or incompatible.

    Examples
    --------
    >>> # Basic configuration with environment variables
    >>> config = DeltaExchangeDataClientConfig()

    >>> # Testnet configuration
    >>> config = DeltaExchangeDataClientConfig.testnet()

    >>> # Custom configuration
    >>> config = DeltaExchangeDataClientConfig(
    ...     api_key="your_api_key",
    ...     api_secret="your_api_secret",
    ...     default_channels=["v2_ticker", "l2_orderbook"],
    ...     symbol_filters=["BTCUSD", "ETHUSD"],
    ...     heartbeat_interval_secs=60
    ... )

    """

    venue: Venue = DELTA_EXCHANGE
    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt = DEFAULT_HTTP_TIMEOUT
    ws_timeout_secs: PositiveInt = DEFAULT_WS_TIMEOUT
    testnet: bool = False
    sandbox: bool = False
    auto_reconnect: bool = True
    reconnection_delay_secs: PositiveInt = DEFAULT_RECONNECTION_DELAY
    max_reconnection_attempts: PositiveInt = DEFAULT_MAX_RECONNECTION_ATTEMPTS
    heartbeat_interval_secs: PositiveInt = DEFAULT_HEARTBEAT_INTERVAL
    max_queue_size: PositiveInt = DEFAULT_MAX_QUEUE_SIZE
    default_channels: list[str] | None = None
    symbol_filters: list[str] | None = None
    rate_limit_requests_per_second: PositiveInt = 75
    log_raw_messages: bool = False

    def __post_init__(self) -> None:
        """Validate configuration after initialization."""
        # Validate mutually exclusive environment flags
        if self.testnet and self.sandbox:
            raise InvalidConfiguration("Cannot enable both testnet and sandbox modes")

        # Validate timeout ranges
        if not (MIN_TIMEOUT_SECS <= self.http_timeout_secs <= MAX_TIMEOUT_SECS):
            raise InvalidConfiguration(
                f"http_timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
            )

        if not (MIN_TIMEOUT_SECS <= self.ws_timeout_secs <= MAX_TIMEOUT_SECS):
            raise InvalidConfiguration(
                f"ws_timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
            )

        # Validate reconnection settings
        if not (MIN_RECONNECTION_DELAY <= self.reconnection_delay_secs <= MAX_RECONNECTION_DELAY):
            raise InvalidConfiguration(
                f"reconnection_delay_secs must be between {MIN_RECONNECTION_DELAY} and {MAX_RECONNECTION_DELAY}"
            )

        # Validate heartbeat interval
        if not (MIN_HEARTBEAT_INTERVAL <= self.heartbeat_interval_secs <= MAX_HEARTBEAT_INTERVAL):
            raise InvalidConfiguration(
                f"heartbeat_interval_secs must be between {MIN_HEARTBEAT_INTERVAL} and {MAX_HEARTBEAT_INTERVAL}"
            )

        # Validate URLs
        _validate_url(self.base_url_http, "HTTP")
        _validate_url(self.base_url_ws, "WebSocket")

        # Validate API credentials format
        _validate_api_credentials(self.api_key, self.api_secret)

        # Validate channels
        if self.default_channels:
            valid_channels = DELTA_EXCHANGE_WS_PUBLIC_CHANNELS + DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
            for channel in self.default_channels:
                if channel not in valid_channels:
                    raise InvalidConfiguration(f"Invalid channel: {channel}")

        # Validate rate limiting
        if self.rate_limit_requests_per_second > 100:
            raise InvalidConfiguration(
                "rate_limit_requests_per_second cannot exceed 100 (Delta Exchange limit)"
            )

    @classmethod
    def testnet(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeDataClientConfig:
        """
        Create a testnet configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_TESTNET_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_TESTNET_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeDataClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,
            base_url_ws=DELTA_EXCHANGE_TESTNET_WS_URL,
            testnet=True,
            **kwargs,
        )

    @classmethod
    def sandbox(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeDataClientConfig:
        """
        Create a sandbox configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_SANDBOX_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_SANDBOX_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeDataClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(sandbox=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,  # Sandbox uses testnet URLs
            base_url_ws=DELTA_EXCHANGE_TESTNET_WS_URL,
            sandbox=True,
            **kwargs,
        )

    @classmethod
    def production(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeDataClientConfig:
        """
        Create a production configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeDataClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=False, sandbox=False)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_BASE_URL,
            base_url_ws=DELTA_EXCHANGE_WS_URL,
            testnet=False,
            sandbox=False,
            **kwargs,
        )

    def get_effective_api_key(self) -> str | None:
        """Get the effective API key, including from environment variables."""
        if self.api_key:
            return self.api_key

        env_key, _ = _get_env_credentials(self.testnet, self.sandbox)
        return env_key

    def get_effective_api_secret(self) -> str | None:
        """Get the effective API secret, including from environment variables."""
        if self.api_secret:
            return self.api_secret

        _, env_secret = _get_env_credentials(self.testnet, self.sandbox)
        return env_secret

    def get_effective_http_url(self) -> str:
        """Get the effective HTTP URL based on environment settings."""
        if self.base_url_http:
            return self.base_url_http

        if self.testnet or self.sandbox:
            return DELTA_EXCHANGE_TESTNET_BASE_URL

        return DELTA_EXCHANGE_BASE_URL

    def get_effective_ws_url(self) -> str:
        """Get the effective WebSocket URL based on environment settings."""
        if self.base_url_ws:
            return self.base_url_ws

        if self.testnet or self.sandbox:
            return DELTA_EXCHANGE_TESTNET_WS_URL

        return DELTA_EXCHANGE_WS_URL

    def has_credentials(self) -> bool:
        """Check if API credentials are available."""
        return (
            self.get_effective_api_key() is not None
            and self.get_effective_api_secret() is not None
        )


class DeltaExchangeExecClientConfig(LiveExecClientConfig, frozen=True):
    """
    Configuration for ``DeltaExchangeExecutionClient`` instances.

    This configuration class provides comprehensive settings for the Delta Exchange
    execution client including API credentials, order management, risk management,
    and trading-specific configurations.

    Parameters
    ----------
    venue : Venue, default DELTA_EXCHANGE
        The venue for the client.
    api_key : str, optional
        The Delta Exchange API key with trading permissions.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_KEY`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_KEY`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_KEY`
    api_secret : str, optional
        The Delta Exchange API secret with trading permissions.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_SECRET`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_SECRET`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_SECRET`
    base_url_http : str, optional
        The HTTP client custom endpoint override.
        Must be a valid HTTP/HTTPS URL.
    base_url_ws : str, optional
        The WebSocket client custom endpoint override.
        Must be a valid WebSocket URL (ws:// or wss://).
    http_timeout_secs : PositiveInt, default 60
        The timeout (seconds) for HTTP requests.
        Must be between 1 and 300 seconds.
    ws_timeout_secs : PositiveInt, default 30
        The timeout (seconds) for WebSocket connections.
        Must be between 1 and 300 seconds.
    testnet : bool, default False
        If the client should connect to the testnet environment.
        Cannot be True if sandbox is True.
    sandbox : bool, default False
        If the client should connect to the sandbox environment.
        Cannot be True if testnet is True.
    default_time_in_force : TimeInForce, default TimeInForce.GTC
        The default time-in-force for orders when not specified.
    post_only_default : bool, default False
        If orders should be post-only by default (maker orders only).
    reduce_only_default : bool, default False
        If orders should be reduce-only by default (position reducing only).
    max_order_size : PositiveFloat, optional
        The maximum order size allowed (in base currency units).
        If None, no client-side limit is enforced.
    max_position_size : PositiveFloat, optional
        The maximum position size allowed (in base currency units).
        If None, no client-side limit is enforced.
    max_notional_per_order : PositiveFloat, optional
        The maximum notional value per order (in quote currency).
        If None, no client-side limit is enforced.
    enable_client_order_id_generation : bool, default True
        If client order IDs should be automatically generated.
    client_order_id_prefix : str, default "NAUTILUS"
        The prefix for generated client order IDs.
    margin_mode : str, default "cross"
        The margin mode for derivatives trading.
        Valid values: "cross", "isolated".
    default_leverage : PositiveFloat, default 1.0
        The default leverage for derivatives positions.
        Must be between 1.0 and the maximum allowed leverage.
    max_leverage : PositiveFloat, default 100.0
        The maximum leverage allowed for any position.
        Must not exceed Delta Exchange limits.
    auto_reduce_only_on_close : bool, default True
        If orders should automatically be set to reduce-only when closing positions.
    enable_position_hedging : bool, default False
        If position hedging should be enabled (allows long and short positions simultaneously).
    max_retries : PositiveInt, default 3
        The maximum number of retries for failed order operations.
    retry_delay_initial_ms : PositiveInt, default 1000
        The initial delay (milliseconds) between retries.
    retry_delay_max_ms : PositiveInt, default 10000
        The maximum delay (milliseconds) between retries.
    rate_limit_requests_per_second : PositiveInt, default 50
        The maximum number of requests per second for order operations.
        Should be lower than data client to prioritize execution.
    enable_order_state_reconciliation : bool, default True
        If order states should be reconciled with the exchange periodically.
    reconciliation_interval_secs : PositiveInt, default 60
        The interval (seconds) for order state reconciliation.
    enable_position_reconciliation : bool, default True
        If positions should be reconciled with the exchange periodically.
    position_reconciliation_interval_secs : PositiveInt, default 30
        The interval (seconds) for position reconciliation.
    log_order_events : bool, default True
        If order events should be logged for audit purposes.

    Raises
    ------
    InvalidConfiguration
        If configuration parameters are invalid or incompatible.

    Examples
    --------
    >>> # Basic configuration with environment variables
    >>> config = DeltaExchangeExecClientConfig()

    >>> # Testnet configuration with custom settings
    >>> config = DeltaExchangeExecClientConfig.testnet(
    ...     default_leverage=10.0,
    ...     max_order_size=1000.0,
    ...     post_only_default=True
    ... )

    >>> # Production configuration with risk limits
    >>> config = DeltaExchangeExecClientConfig.production(
    ...     api_key="your_api_key",
    ...     api_secret="your_api_secret",
    ...     max_position_size=10000.0,
    ...     max_notional_per_order=50000.0,
    ...     margin_mode="isolated"
    ... )

    """

    venue: Venue = DELTA_EXCHANGE
    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    base_url_ws: str | None = None
    http_timeout_secs: PositiveInt = DEFAULT_HTTP_TIMEOUT
    ws_timeout_secs: PositiveInt = DEFAULT_WS_TIMEOUT
    testnet: bool = False
    sandbox: bool = False
    default_time_in_force: TimeInForce = TimeInForce.GTC
    post_only_default: bool = False
    reduce_only_default: bool = False
    max_order_size: PositiveFloat | None = None
    max_position_size: PositiveFloat | None = None
    max_notional_per_order: PositiveFloat | None = None
    enable_client_order_id_generation: bool = True
    client_order_id_prefix: str = "NAUTILUS"
    margin_mode: str = "cross"
    default_leverage: PositiveFloat = 1.0
    max_leverage: PositiveFloat = 100.0
    auto_reduce_only_on_close: bool = True
    enable_position_hedging: bool = False
    max_retries: PositiveInt = 3
    retry_delay_initial_ms: PositiveInt = 1000
    retry_delay_max_ms: PositiveInt = 10000
    rate_limit_requests_per_second: PositiveInt = 50
    enable_order_state_reconciliation: bool = True
    reconciliation_interval_secs: PositiveInt = 60
    enable_position_reconciliation: bool = True
    position_reconciliation_interval_secs: PositiveInt = 30
    log_order_events: bool = True

    def __post_init__(self) -> None:
        """Validate configuration after initialization."""
        # Validate mutually exclusive environment flags
        if self.testnet and self.sandbox:
            raise InvalidConfiguration("Cannot enable both testnet and sandbox modes")

        # Validate timeout ranges
        if not (MIN_TIMEOUT_SECS <= self.http_timeout_secs <= MAX_TIMEOUT_SECS):
            raise InvalidConfiguration(
                f"http_timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
            )

        if not (MIN_TIMEOUT_SECS <= self.ws_timeout_secs <= MAX_TIMEOUT_SECS):
            raise InvalidConfiguration(
                f"ws_timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
            )

        # Validate URLs
        _validate_url(self.base_url_http, "HTTP")
        _validate_url(self.base_url_ws, "WebSocket")

        # Validate API credentials format
        _validate_api_credentials(self.api_key, self.api_secret)

        # Validate margin mode
        if self.margin_mode not in ("cross", "isolated"):
            raise InvalidConfiguration(f"Invalid margin_mode: {self.margin_mode}")

        # Validate leverage settings
        if self.default_leverage > self.max_leverage:
            raise InvalidConfiguration(
                f"default_leverage ({self.default_leverage}) cannot exceed max_leverage ({self.max_leverage})"
            )

        if self.max_leverage > 200.0:  # Delta Exchange typical maximum
            raise InvalidConfiguration(
                f"max_leverage ({self.max_leverage}) exceeds typical Delta Exchange limits"
            )

        # Validate retry settings
        if self.retry_delay_initial_ms > self.retry_delay_max_ms:
            raise InvalidConfiguration(
                "retry_delay_initial_ms cannot exceed retry_delay_max_ms"
            )

        # Validate rate limiting
        if self.rate_limit_requests_per_second > 75:
            raise InvalidConfiguration(
                "rate_limit_requests_per_second should not exceed 75 for execution client"
            )

        # Validate client order ID prefix
        if not self.client_order_id_prefix or len(self.client_order_id_prefix) > 20:
            raise InvalidConfiguration(
                "client_order_id_prefix must be 1-20 characters long"
            )

        # Validate reconciliation intervals
        if self.reconciliation_interval_secs < 10:
            raise InvalidConfiguration(
                "reconciliation_interval_secs must be at least 10 seconds"
            )

        if self.position_reconciliation_interval_secs < 10:
            raise InvalidConfiguration(
                "position_reconciliation_interval_secs must be at least 10 seconds"
            )

    @classmethod
    def testnet(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeExecClientConfig:
        """
        Create a testnet configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_TESTNET_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_TESTNET_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeExecClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,
            base_url_ws=DELTA_EXCHANGE_TESTNET_WS_URL,
            testnet=True,
            **kwargs,
        )

    @classmethod
    def sandbox(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeExecClientConfig:
        """
        Create a sandbox configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_SANDBOX_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_SANDBOX_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeExecClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(sandbox=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,  # Sandbox uses testnet URLs
            base_url_ws=DELTA_EXCHANGE_TESTNET_WS_URL,
            sandbox=True,
            **kwargs,
        )

    @classmethod
    def production(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeExecClientConfig:
        """
        Create a production configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeExecClientConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=False, sandbox=False)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_BASE_URL,
            base_url_ws=DELTA_EXCHANGE_WS_URL,
            testnet=False,
            sandbox=False,
            **kwargs,
        )

    def get_effective_api_key(self) -> str | None:
        """Get the effective API key, including from environment variables."""
        if self.api_key:
            return self.api_key

        env_key, _ = _get_env_credentials(self.testnet, self.sandbox)
        return env_key

    def get_effective_api_secret(self) -> str | None:
        """Get the effective API secret, including from environment variables."""
        if self.api_secret:
            return self.api_secret

        _, env_secret = _get_env_credentials(self.testnet, self.sandbox)
        return env_secret

    def get_effective_http_url(self) -> str:
        """Get the effective HTTP URL based on environment settings."""
        if self.base_url_http:
            return self.base_url_http

        if self.testnet or self.sandbox:
            return DELTA_EXCHANGE_TESTNET_BASE_URL

        return DELTA_EXCHANGE_BASE_URL

    def get_effective_ws_url(self) -> str:
        """Get the effective WebSocket URL based on environment settings."""
        if self.base_url_ws:
            return self.base_url_ws

        if self.testnet or self.sandbox:
            return DELTA_EXCHANGE_TESTNET_WS_URL

        return DELTA_EXCHANGE_WS_URL

    def has_credentials(self) -> bool:
        """Check if API credentials are available."""
        return (
            self.get_effective_api_key() is not None
            and self.get_effective_api_secret() is not None
        )

    def validate_risk_parameters(self) -> bool:
        """
        Validate risk management parameters.

        Returns
        -------
        bool
            True if all risk parameters are valid.

        Raises
        ------
        InvalidConfiguration
            If any risk parameter is invalid.

        """
        # Check for reasonable risk limits
        if self.max_order_size and self.max_order_size <= 0:
            raise InvalidConfiguration("max_order_size must be positive")

        if self.max_position_size and self.max_position_size <= 0:
            raise InvalidConfiguration("max_position_size must be positive")

        if self.max_notional_per_order and self.max_notional_per_order <= 0:
            raise InvalidConfiguration("max_notional_per_order must be positive")

        # Validate leverage is reasonable
        if self.default_leverage < 1.0:
            raise InvalidConfiguration("default_leverage must be at least 1.0")

        return True

    def get_order_retry_config(self) -> dict[str, Any]:
        """
        Get order retry configuration.

        Returns
        -------
        dict[str, Any]
            Dictionary containing retry configuration.

        """
        return {
            "max_retries": self.max_retries,
            "initial_delay_ms": self.retry_delay_initial_ms,
            "max_delay_ms": self.retry_delay_max_ms,
        }


class DeltaExchangeInstrumentProviderConfig(InstrumentProviderConfig, frozen=True):
    """
    Configuration for ``DeltaExchangeInstrumentProvider`` instances.

    This configuration class provides comprehensive settings for loading and managing
    Delta Exchange instruments with proper filtering, caching, and refresh mechanisms.

    Parameters
    ----------
    api_key : str, optional
        The Delta Exchange API key.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_KEY`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_KEY`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_KEY`
    api_secret : str, optional
        The Delta Exchange API secret.
        If ``None`` then will source from environment variables:
        - Production: `DELTA_EXCHANGE_API_SECRET`
        - Testnet: `DELTA_EXCHANGE_TESTNET_API_SECRET`
        - Sandbox: `DELTA_EXCHANGE_SANDBOX_API_SECRET`
    base_url_http : str, optional
        The HTTP client custom endpoint override.
        Must be a valid HTTP/HTTPS URL.
    http_timeout_secs : PositiveInt, default 60
        The timeout (seconds) for HTTP requests.
        Must be between 1 and 300 seconds.
    testnet : bool, default False
        If the provider should connect to the testnet environment.
        Cannot be True if sandbox is True.
    sandbox : bool, default False
        If the provider should connect to the sandbox environment.
        Cannot be True if testnet is True.
    product_types : list[str], optional
        The Delta Exchange product types to load.
        Valid values: "perpetual_futures", "call_options", "put_options".
        If None, all product types are loaded.
    load_active_only : bool, default True
        If only active/tradeable instruments should be loaded.
    load_expired : bool, default False
        If expired instruments should be included.
    symbol_filters : list[str], optional
        Symbol patterns to filter instruments (e.g., ["BTC*", "ETH*"]).
        Supports glob-style patterns with * and ? wildcards.
    trading_status_filters : list[str], optional
        Trading status filters to apply.
        Valid values depend on Delta Exchange API response.
    cache_validity_hours : PositiveInt, default 24
        The number of hours instrument data remains valid in cache.
        After this period, instruments will be refreshed from the API.
    update_instruments_interval_mins : PositiveInt, default 60
        The interval (minutes) between automatic instrument updates.
        Must be between 1 and 1440 (24 hours).
    enable_auto_refresh : bool, default True
        If instruments should be automatically refreshed at intervals.
    refresh_on_start : bool, default True
        If instruments should be refreshed when the provider starts.
    max_concurrent_requests : PositiveInt, default 5
        The maximum number of concurrent HTTP requests for loading instruments.
    request_delay_ms : NonNegativeInt, default 100
        The delay (milliseconds) between instrument loading requests.
    enable_instrument_caching : bool, default True
        If loaded instruments should be cached to disk.
    cache_directory : str, optional
        The directory path for instrument cache files.
        If None, uses system temporary directory.
    cache_file_prefix : str, default "delta_exchange_instruments"
        The prefix for cache file names.
    log_instrument_loading : bool, default True
        If instrument loading progress should be logged.

    Raises
    ------
    InvalidConfiguration
        If configuration parameters are invalid or incompatible.

    Examples
    --------
    >>> # Basic configuration
    >>> config = DeltaExchangeInstrumentProviderConfig()

    >>> # Load only perpetual futures for BTC and ETH
    >>> config = DeltaExchangeInstrumentProviderConfig(
    ...     product_types=["perpetual_futures"],
    ...     symbol_filters=["BTC*", "ETH*"],
    ...     update_instruments_interval_mins=30
    ... )

    >>> # Testnet configuration with custom caching
    >>> config = DeltaExchangeInstrumentProviderConfig.testnet(
    ...     cache_validity_hours=12,
    ...     cache_directory="/custom/cache/path",
    ...     load_expired=True
    ... )

    """

    api_key: str | None = None
    api_secret: str | None = None
    base_url_http: str | None = None
    http_timeout_secs: PositiveInt = DEFAULT_HTTP_TIMEOUT
    testnet: bool = False
    sandbox: bool = False
    product_types: list[str] | None = None
    load_active_only: bool = True
    load_expired: bool = False
    symbol_filters: list[str] | None = None
    trading_status_filters: list[str] | None = None
    cache_validity_hours: PositiveInt = DEFAULT_CACHE_VALIDITY_HOURS
    update_instruments_interval_mins: PositiveInt = DEFAULT_UPDATE_INSTRUMENTS_INTERVAL
    enable_auto_refresh: bool = True
    refresh_on_start: bool = True
    max_concurrent_requests: PositiveInt = 5
    request_delay_ms: NonNegativeInt = 100
    enable_instrument_caching: bool = True
    cache_directory: str | None = None
    cache_file_prefix: str = "delta_exchange_instruments"
    log_instrument_loading: bool = True

    def __post_init__(self) -> None:
        """Validate configuration after initialization."""
        # Validate mutually exclusive environment flags
        if self.testnet and self.sandbox:
            raise InvalidConfiguration("Cannot enable both testnet and sandbox modes")

        # Validate timeout range
        if not (MIN_TIMEOUT_SECS <= self.http_timeout_secs <= MAX_TIMEOUT_SECS):
            raise InvalidConfiguration(
                f"http_timeout_secs must be between {MIN_TIMEOUT_SECS} and {MAX_TIMEOUT_SECS}"
            )

        # Validate URL
        _validate_url(self.base_url_http, "HTTP")

        # Validate API credentials format
        _validate_api_credentials(self.api_key, self.api_secret)

        # Validate product types
        if self.product_types:
            for product_type in self.product_types:
                if product_type not in DELTA_EXCHANGE_PRODUCT_TYPES:
                    raise InvalidConfiguration(
                        f"Invalid product_type: {product_type}. "
                        f"Valid types: {DELTA_EXCHANGE_PRODUCT_TYPES}"
                    )

        # Validate update interval
        if not (MIN_UPDATE_INTERVAL <= self.update_instruments_interval_mins <= MAX_UPDATE_INTERVAL):
            raise InvalidConfiguration(
                f"update_instruments_interval_mins must be between {MIN_UPDATE_INTERVAL} and {MAX_UPDATE_INTERVAL}"
            )

        # Validate cache validity
        if self.cache_validity_hours <= 0 or self.cache_validity_hours > 168:  # 1 week max
            raise InvalidConfiguration(
                "cache_validity_hours must be between 1 and 168 (1 week)"
            )

        # Validate concurrent requests
        if self.max_concurrent_requests > 20:
            raise InvalidConfiguration(
                "max_concurrent_requests should not exceed 20 to avoid rate limiting"
            )

        # Validate cache file prefix
        if not self.cache_file_prefix or len(self.cache_file_prefix) > 50:
            raise InvalidConfiguration(
                "cache_file_prefix must be 1-50 characters long"
            )

        # Validate symbol filters format
        if self.symbol_filters:
            for pattern in self.symbol_filters:
                if not pattern or len(pattern) > 50:
                    raise InvalidConfiguration(
                        "Symbol filter patterns must be 1-50 characters long"
                    )

    @classmethod
    def testnet(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeInstrumentProviderConfig:
        """
        Create a testnet configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_TESTNET_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_TESTNET_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeInstrumentProviderConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,
            testnet=True,
            **kwargs,
        )

    @classmethod
    def sandbox(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeInstrumentProviderConfig:
        """
        Create a sandbox configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_SANDBOX_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_SANDBOX_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeInstrumentProviderConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(sandbox=True)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_TESTNET_BASE_URL,  # Sandbox uses testnet URLs
            sandbox=True,
            **kwargs,
        )

    @classmethod
    def production(
        cls,
        api_key: str | None = None,
        api_secret: str | None = None,
        **kwargs: Any,
    ) -> DeltaExchangeInstrumentProviderConfig:
        """
        Create a production configuration.

        Parameters
        ----------
        api_key : str, optional
            The API key. If None, will use DELTA_EXCHANGE_API_KEY.
        api_secret : str, optional
            The API secret. If None, will use DELTA_EXCHANGE_API_SECRET.
        **kwargs
            Additional configuration parameters.

        Returns
        -------
        DeltaExchangeInstrumentProviderConfig

        """
        if api_key is None or api_secret is None:
            env_key, env_secret = _get_env_credentials(testnet=False, sandbox=False)
            api_key = api_key or env_key
            api_secret = api_secret or env_secret

        return cls(
            api_key=api_key,
            api_secret=api_secret,
            base_url_http=DELTA_EXCHANGE_BASE_URL,
            testnet=False,
            sandbox=False,
            **kwargs,
        )

    def get_effective_api_key(self) -> str | None:
        """Get the effective API key, including from environment variables."""
        if self.api_key:
            return self.api_key

        env_key, _ = _get_env_credentials(self.testnet, self.sandbox)
        return env_key

    def get_effective_api_secret(self) -> str | None:
        """Get the effective API secret, including from environment variables."""
        if self.api_secret:
            return self.api_secret

        _, env_secret = _get_env_credentials(self.testnet, self.sandbox)
        return env_secret

    def get_effective_http_url(self) -> str:
        """Get the effective HTTP URL based on environment settings."""
        if self.base_url_http:
            return self.base_url_http

        if self.testnet or self.sandbox:
            return DELTA_EXCHANGE_TESTNET_BASE_URL

        return DELTA_EXCHANGE_BASE_URL

    def has_credentials(self) -> bool:
        """Check if API credentials are available."""
        return (
            self.get_effective_api_key() is not None
            and self.get_effective_api_secret() is not None
        )

    def get_product_type_filters(self) -> list[str]:
        """
        Get the effective product type filters.

        Returns
        -------
        list[str]
            List of product types to load, or all types if none specified.

        """
        return self.product_types or DELTA_EXCHANGE_PRODUCT_TYPES.copy()

    def should_load_instrument(self, symbol: str, product_type: str, trading_status: str) -> bool:
        """
        Check if an instrument should be loaded based on filters.

        Parameters
        ----------
        symbol : str
            The instrument symbol.
        product_type : str
            The product type.
        trading_status : str
            The trading status.

        Returns
        -------
        bool
            True if the instrument should be loaded.

        """
        # Check product type filter
        if self.product_types and product_type not in self.product_types:
            return False

        # Check symbol filters
        if self.symbol_filters:
            import fnmatch
            if not any(fnmatch.fnmatch(symbol, pattern) for pattern in self.symbol_filters):
                return False

        # Check trading status filters
        if self.trading_status_filters and trading_status not in self.trading_status_filters:
            return False

        # Check active only filter
        if self.load_active_only and trading_status.lower() not in ("active", "trading"):
            return False

        return True

    def get_cache_file_path(self) -> str:
        """
        Get the full path for the instrument cache file.

        Returns
        -------
        str
            The cache file path.

        """
        import tempfile
        import os.path

        cache_dir = self.cache_directory or tempfile.gettempdir()
        env_suffix = ""
        if self.testnet:
            env_suffix = "_testnet"
        elif self.sandbox:
            env_suffix = "_sandbox"

        filename = f"{self.cache_file_prefix}{env_suffix}.json"
        return os.path.join(cache_dir, filename)

    def is_cache_valid(self) -> bool:
        """
        Check if the instrument cache is still valid.

        Returns
        -------
        bool
            True if the cache is valid and can be used.

        """
        if not self.enable_instrument_caching:
            return False

        import os
        import time

        cache_path = self.get_cache_file_path()
        if not os.path.exists(cache_path):
            return False

        # Check if cache is within validity period
        cache_age_hours = (time.time() - os.path.getmtime(cache_path)) / 3600
        return cache_age_hours < self.cache_validity_hours
