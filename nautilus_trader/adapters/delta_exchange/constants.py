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
Delta Exchange Constants and Enumerations.

This module provides comprehensive constants and enumerations for the Delta Exchange
adapter, including venue identifiers, API endpoints, WebSocket channels, order types,
trading statuses, error codes, and data model mappings. All constants follow
established Nautilus Trader patterns and provide type safety for Delta Exchange
integration.
"""

from enum import Enum, unique
from typing import Final

from nautilus_trader.model.enums import OrderSide, OrderStatus, OrderType, TimeInForce
from nautilus_trader.model.identifiers import ClientId, Venue


# -- VENUE IDENTIFIERS AND CORE CONSTANTS ----------------------------------------------------

DELTA_EXCHANGE: Final[str] = "DELTA_EXCHANGE"
DELTA_EXCHANGE_VENUE: Final[Venue] = Venue(DELTA_EXCHANGE)
DELTA_EXCHANGE_CLIENT_ID: Final[ClientId] = ClientId(DELTA_EXCHANGE)

# -- API URLS AND ENDPOINTS ------------------------------------------------------------------

# Base URLs for different environments
DELTA_EXCHANGE_BASE_URL: Final[str] = "https://api.delta.exchange"
DELTA_EXCHANGE_TESTNET_BASE_URL: Final[str] = "https://testnet-api.delta.exchange"
DELTA_EXCHANGE_SANDBOX_BASE_URL: Final[str] = "https://sandbox-api.delta.exchange"

# WebSocket URLs for different environments
DELTA_EXCHANGE_WS_URL: Final[str] = "wss://socket.delta.exchange"
DELTA_EXCHANGE_TESTNET_WS_URL: Final[str] = "wss://testnet-socket.delta.exchange"
DELTA_EXCHANGE_SANDBOX_WS_URL: Final[str] = "wss://sandbox-socket.delta.exchange"

# API Version
DELTA_EXCHANGE_API_VERSION: Final[str] = "v2"

# REST API Endpoints
DELTA_EXCHANGE_PRODUCTS_ENDPOINT: Final[str] = "/v2/products"
DELTA_EXCHANGE_ASSETS_ENDPOINT: Final[str] = "/v2/assets"
DELTA_EXCHANGE_ORDERS_ENDPOINT: Final[str] = "/v2/orders"
DELTA_EXCHANGE_BATCH_ORDERS_ENDPOINT: Final[str] = "/v2/orders/batch"
DELTA_EXCHANGE_POSITIONS_ENDPOINT: Final[str] = "/v2/positions"
DELTA_EXCHANGE_WALLET_ENDPOINT: Final[str] = "/v2/wallet"
DELTA_EXCHANGE_BALANCES_ENDPOINT: Final[str] = "/v2/wallet/balances"
DELTA_EXCHANGE_FILLS_ENDPOINT: Final[str] = "/v2/fills"
DELTA_EXCHANGE_ORDERBOOK_ENDPOINT: Final[str] = "/v2/l2orderbook"
DELTA_EXCHANGE_TRADES_ENDPOINT: Final[str] = "/v2/trades"
DELTA_EXCHANGE_TICKERS_ENDPOINT: Final[str] = "/v2/tickers"
DELTA_EXCHANGE_CANDLES_ENDPOINT: Final[str] = "/v2/history/candles"
DELTA_EXCHANGE_MARK_PRICE_ENDPOINT: Final[str] = "/v2/mark_price"
DELTA_EXCHANGE_FUNDING_RATE_ENDPOINT: Final[str] = "/v2/funding_rate"
DELTA_EXCHANGE_ACCOUNT_ENDPOINT: Final[str] = "/v2/profile"
DELTA_EXCHANGE_LEVERAGE_ENDPOINT: Final[str] = "/v2/positions/leverage"
DELTA_EXCHANGE_MARGINS_ENDPOINT: Final[str] = "/v2/margins"

# Environment URL mappings
DELTA_EXCHANGE_HTTP_URLS: Final[dict[str, str]] = {
    "production": DELTA_EXCHANGE_BASE_URL,
    "testnet": DELTA_EXCHANGE_TESTNET_BASE_URL,
    "sandbox": DELTA_EXCHANGE_SANDBOX_BASE_URL,
}

DELTA_EXCHANGE_WS_URLS: Final[dict[str, str]] = {
    "production": DELTA_EXCHANGE_WS_URL,
    "testnet": DELTA_EXCHANGE_TESTNET_WS_URL,
    "sandbox": DELTA_EXCHANGE_SANDBOX_WS_URL,
}

# -- WEBSOCKET CONSTANTS ---------------------------------------------------------------------

# Public WebSocket Channels
DELTA_EXCHANGE_WS_PUBLIC_CHANNELS: Final[list[str]] = [
    "v2_ticker",           # Real-time ticker updates
    "l1_orderbook",        # Level 1 order book (best bid/ask)
    "l2_orderbook",        # Level 2 order book snapshots
    "l2_updates",          # Level 2 order book incremental updates
    "all_trades",          # All public trades
    "mark_price",          # Mark price updates
    "candlesticks",        # OHLCV candlestick data
    "spot_price",          # Spot price updates
    "v2/spot_price",       # V2 spot price updates
    "spot_30mtwap_price",  # 30-minute TWAP spot price
    "funding_rate",        # Funding rate updates
    "product_updates",     # Product/instrument updates
    "announcements",       # Exchange announcements
]

# Private WebSocket Channels (require authentication)
DELTA_EXCHANGE_WS_PRIVATE_CHANNELS: Final[list[str]] = [
    "margins",             # Margin updates
    "positions",           # Position updates
    "orders",              # Order status updates
    "user_trades",         # User trade executions
    "v2/user_trades",      # V2 user trade executions
    "portfolio_margins",   # Portfolio margin updates
    "mmp_trigger",         # Market Maker Protection triggers
]

# All supported WebSocket channels
DELTA_EXCHANGE_WS_ALL_CHANNELS: Final[list[str]] = (
    DELTA_EXCHANGE_WS_PUBLIC_CHANNELS + DELTA_EXCHANGE_WS_PRIVATE_CHANNELS
)

# WebSocket message types
DELTA_EXCHANGE_WS_MESSAGE_TYPES: Final[dict[str, str]] = {
    "subscribe": "subscribe",
    "unsubscribe": "unsubscribe",
    "auth": "auth",
    "ping": "ping",
    "pong": "pong",
}

# WebSocket connection states
DELTA_EXCHANGE_WS_CONNECTION_STATES: Final[dict[str, str]] = {
    "connecting": "connecting",
    "connected": "connected",
    "authenticated": "authenticated",
    "disconnecting": "disconnecting",
    "disconnected": "disconnected",
    "error": "error",
}

# -- DELTA EXCHANGE ENUMERATIONS -------------------------------------------------------------

@unique
class DeltaExchangeProductType(Enum):
    """Delta Exchange product types."""

    PERPETUAL_FUTURES = "perpetual_futures"
    CALL_OPTIONS = "call_options"
    PUT_OPTIONS = "put_options"

    @property
    def is_perpetual(self) -> bool:
        """Check if product type is perpetual futures."""
        return self == DeltaExchangeProductType.PERPETUAL_FUTURES

    @property
    def is_option(self) -> bool:
        """Check if product type is an option."""
        return self in (DeltaExchangeProductType.CALL_OPTIONS, DeltaExchangeProductType.PUT_OPTIONS)


@unique
class DeltaExchangeOrderType(Enum):
    """Delta Exchange order types."""

    LIMIT_ORDER = "limit_order"
    MARKET_ORDER = "market_order"
    STOP_LOSS_ORDER = "stop_loss_order"
    TAKE_PROFIT_ORDER = "take_profit_order"

    @property
    def is_market(self) -> bool:
        """Check if order type is market."""
        return self == DeltaExchangeOrderType.MARKET_ORDER

    @property
    def is_limit(self) -> bool:
        """Check if order type is limit."""
        return self == DeltaExchangeOrderType.LIMIT_ORDER

    @property
    def is_stop(self) -> bool:
        """Check if order type is stop-related."""
        return self in (DeltaExchangeOrderType.STOP_LOSS_ORDER, DeltaExchangeOrderType.TAKE_PROFIT_ORDER)


@unique
class DeltaExchangeOrderStatus(Enum):
    """Delta Exchange order status values."""

    OPEN = "open"
    PENDING = "pending"
    CLOSED = "closed"
    CANCELLED = "cancelled"
    REJECTED = "rejected"
    EXPIRED = "expired"
    FILLED = "filled"
    PARTIALLY_FILLED = "partially_filled"

    @property
    def is_active(self) -> bool:
        """Check if order status indicates an active order."""
        return self in (DeltaExchangeOrderStatus.OPEN, DeltaExchangeOrderStatus.PENDING, DeltaExchangeOrderStatus.PARTIALLY_FILLED)

    @property
    def is_terminal(self) -> bool:
        """Check if order status is terminal (final)."""
        return self in (DeltaExchangeOrderStatus.CLOSED, DeltaExchangeOrderStatus.CANCELLED,
                       DeltaExchangeOrderStatus.REJECTED, DeltaExchangeOrderStatus.EXPIRED, DeltaExchangeOrderStatus.FILLED)


@unique
class DeltaExchangeTimeInForce(Enum):
    """Delta Exchange time-in-force values."""

    GTC = "gtc"  # Good Till Cancel
    IOC = "ioc"  # Immediate or Cancel
    FOK = "fok"  # Fill or Kill
    GTD = "gtd"  # Good Till Date

    @property
    def is_immediate(self) -> bool:
        """Check if time-in-force requires immediate execution."""
        return self in (DeltaExchangeTimeInForce.IOC, DeltaExchangeTimeInForce.FOK)


@unique
class DeltaExchangeOrderSide(Enum):
    """Delta Exchange order side values."""

    BUY = "buy"
    SELL = "sell"


@unique
class DeltaExchangeTradingStatus(Enum):
    """Delta Exchange instrument trading status."""

    ACTIVE = "active"
    INACTIVE = "inactive"
    EXPIRED = "expired"
    SUSPENDED = "suspended"
    DELISTED = "delisted"

    @property
    def is_tradable(self) -> bool:
        """Check if trading status allows trading."""
        return self == DeltaExchangeTradingStatus.ACTIVE

# -- API RESPONSE AND ERROR CONSTANTS -------------------------------------------------------

# API Response Status
DELTA_EXCHANGE_API_SUCCESS: Final[str] = "success"
DELTA_EXCHANGE_API_ERROR: Final[str] = "error"

# Common API Error Codes
DELTA_EXCHANGE_ERROR_CODES: Final[dict[int, str]] = {
    400: "Bad Request",
    401: "Unauthorized",
    403: "Forbidden",
    404: "Not Found",
    429: "Too Many Requests",
    500: "Internal Server Error",
    502: "Bad Gateway",
    503: "Service Unavailable",
    504: "Gateway Timeout",
    1001: "Invalid API Key",
    1002: "Invalid Signature",
    1003: "Invalid Timestamp",
    1004: "Invalid Nonce",
    2001: "Insufficient Balance",
    2002: "Order Not Found",
    2003: "Invalid Order Size",
    2004: "Invalid Order Price",
    2005: "Order Already Cancelled",
    3001: "Position Not Found",
    3002: "Invalid Position Size",
    3003: "Insufficient Margin",
    4001: "Product Not Found",
    4002: "Product Not Active",
    4003: "Trading Suspended",
}

# Rate Limiting Constants
DELTA_EXCHANGE_MAX_REQUESTS_PER_SECOND: Final[int] = 100
DELTA_EXCHANGE_WS_MAX_CONNECTIONS_PER_IP: Final[int] = 150
DELTA_EXCHANGE_RATE_LIMIT_HEADER: Final[str] = "X-RateLimit-Remaining"
DELTA_EXCHANGE_RATE_LIMIT_RESET_HEADER: Final[str] = "X-RateLimit-Reset"

# Pagination Constants
DELTA_EXCHANGE_MAX_PAGE_SIZE: Final[int] = 100
DELTA_EXCHANGE_DEFAULT_PAGE_SIZE: Final[int] = 50
DELTA_EXCHANGE_MIN_PAGE_SIZE: Final[int] = 1

# Timeout Constants (in seconds)
DELTA_EXCHANGE_DEFAULT_HTTP_TIMEOUT: Final[int] = 60
DELTA_EXCHANGE_DEFAULT_WS_TIMEOUT: Final[int] = 10
DELTA_EXCHANGE_DEFAULT_CONNECT_TIMEOUT: Final[int] = 30
DELTA_EXCHANGE_DEFAULT_READ_TIMEOUT: Final[int] = 60

# Precision Constants
DELTA_EXCHANGE_TIMESTAMP_PRECISION: Final[int] = 6  # Microseconds
DELTA_EXCHANGE_PRICE_PRECISION: Final[int] = 8     # Decimal places
DELTA_EXCHANGE_QUANTITY_PRECISION: Final[int] = 8  # Decimal places


# -- TRADING AND RISK MANAGEMENT CONSTANTS ---------------------------------------------------

# Default Risk Parameters
DELTA_EXCHANGE_DEFAULT_POSITION_LIMIT: Final[str] = "1000000.0"  # Default position limit
DELTA_EXCHANGE_DEFAULT_DAILY_LOSS_LIMIT: Final[str] = "100000.0"  # Default daily loss limit
DELTA_EXCHANGE_DEFAULT_MAX_POSITION_VALUE: Final[str] = "10000000.0"  # Default max position value

# Order Size Limits
DELTA_EXCHANGE_MIN_ORDER_SIZE: Final[str] = "0.000001"  # Minimum order size
DELTA_EXCHANGE_MAX_ORDER_SIZE: Final[str] = "1000000.0"  # Maximum order size

# Margin Requirements
DELTA_EXCHANGE_MIN_MARGIN_RATIO: Final[str] = "0.01"  # 1% minimum margin
DELTA_EXCHANGE_DEFAULT_LEVERAGE: Final[int] = 1       # Default leverage
DELTA_EXCHANGE_MAX_LEVERAGE: Final[int] = 100         # Maximum leverage

# Fee Structure (in basis points)
DELTA_EXCHANGE_DEFAULT_MAKER_FEE: Final[str] = "0.0002"  # 0.02% maker fee
DELTA_EXCHANGE_DEFAULT_TAKER_FEE: Final[str] = "0.0005"  # 0.05% taker fee

# Market Maker Protection (MMP) Constants
DELTA_EXCHANGE_MMP_DEFAULT_DELTA_LIMIT: Final[str] = "1000.0"
DELTA_EXCHANGE_MMP_DEFAULT_VEGA_LIMIT: Final[str] = "10000.0"
DELTA_EXCHANGE_MMP_DEFAULT_FROZEN_TIME: Final[int] = 5  # seconds


# -- SUPPORTED ORDER TYPES AND TIME IN FORCE ------------------------------------------------

# Nautilus Trader supported order types for Delta Exchange
DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES: Final[set[OrderType]] = {
    OrderType.MARKET,
    OrderType.LIMIT,
    OrderType.STOP_MARKET,
    OrderType.STOP_LIMIT,
}

# Nautilus Trader supported time-in-force for Delta Exchange
DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE: Final[set[TimeInForce]] = {
    TimeInForce.GTC,  # Good Till Cancel
    TimeInForce.IOC,  # Immediate or Cancel
    TimeInForce.FOK,  # Fill or Kill
    TimeInForce.GTD,  # Good Till Date
}

# Nautilus Trader supported order sides for Delta Exchange
DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES: Final[set[OrderSide]] = {
    OrderSide.BUY,
    OrderSide.SELL,
}


# -- DATA MODEL MAPPING CONSTANTS ------------------------------------------------------------

# Delta Exchange to Nautilus OrderType mapping
DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE: Final[dict[str, OrderType]] = {
    "limit_order": OrderType.LIMIT,
    "market_order": OrderType.MARKET,
    "stop_loss_order": OrderType.STOP_MARKET,
    "take_profit_order": OrderType.STOP_LIMIT,
}

# Nautilus to Delta Exchange OrderType mapping
NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE: Final[dict[OrderType, str]] = {
    OrderType.LIMIT: "limit_order",
    OrderType.MARKET: "market_order",
    OrderType.STOP_MARKET: "stop_loss_order",
    OrderType.STOP_LIMIT: "take_profit_order",
}

# Delta Exchange to Nautilus OrderStatus mapping
DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS: Final[dict[str, OrderStatus]] = {
    "open": OrderStatus.ACCEPTED,
    "pending": OrderStatus.PENDING_NEW,
    "closed": OrderStatus.FILLED,
    "cancelled": OrderStatus.CANCELED,
    "rejected": OrderStatus.REJECTED,
    "expired": OrderStatus.EXPIRED,
    "filled": OrderStatus.FILLED,
    "partially_filled": OrderStatus.PARTIALLY_FILLED,
}

# Delta Exchange to Nautilus TimeInForce mapping
DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE: Final[dict[str, TimeInForce]] = {
    "gtc": TimeInForce.GTC,
    "ioc": TimeInForce.IOC,
    "fok": TimeInForce.FOK,
    "gtd": TimeInForce.GTD,
}

# Nautilus to Delta Exchange TimeInForce mapping
NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE: Final[dict[TimeInForce, str]] = {
    TimeInForce.GTC: "gtc",
    TimeInForce.IOC: "ioc",
    TimeInForce.FOK: "fok",
    TimeInForce.GTD: "gtd",
}

# Delta Exchange to Nautilus OrderSide mapping
DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE: Final[dict[str, OrderSide]] = {
    "buy": OrderSide.BUY,
    "sell": OrderSide.SELL,
}

# Nautilus to Delta Exchange OrderSide mapping
NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE: Final[dict[OrderSide, str]] = {
    OrderSide.BUY: "buy",
    OrderSide.SELL: "sell",
}

# Currency code mappings (Delta Exchange to standard codes)
DELTA_EXCHANGE_CURRENCY_MAP: Final[dict[str, str]] = {
    "USDT": "USDT",
    "USDC": "USDC",
    "BTC": "BTC",
    "ETH": "ETH",
    "SOL": "SOL",
    "MATIC": "MATIC",
    "AVAX": "AVAX",
    "BNB": "BNB",
    "ADA": "ADA",
    "DOT": "DOT",
    "LINK": "LINK",
    "UNI": "UNI",
    "AAVE": "AAVE",
    "SUSHI": "SUSHI",
    "CRV": "CRV",
    "YFI": "YFI",
    "COMP": "COMP",
    "MKR": "MKR",
    "SNX": "SNX",
}

# Product type mappings
DELTA_EXCHANGE_PRODUCT_TYPE_MAP: Final[dict[str, str]] = {
    "perpetual_futures": "PERPETUAL",
    "call_options": "CALL_OPTION",
    "put_options": "PUT_OPTION",
}


# -- ENVIRONMENT AND CONFIGURATION CONSTANTS -------------------------------------------------

# Environment identifiers
DELTA_EXCHANGE_ENVIRONMENTS: Final[list[str]] = ["production", "testnet", "sandbox"]

# Default configuration values
DELTA_EXCHANGE_DEFAULT_CONFIG: Final[dict[str, any]] = {
    "testnet": False,
    "sandbox": False,
    "enable_private_channels": True,
    "product_types": ["perpetual_futures"],
    "symbol_patterns": ["*"],
    "max_retries": 3,
    "retry_delay_secs": 1.0,
    "heartbeat_interval_secs": 30.0,
    "request_timeout_secs": 60.0,
    "ws_timeout_secs": 10.0,
    "max_reconnect_attempts": 10,
    "reconnect_delay_secs": 5.0,
}

# Feature flags
DELTA_EXCHANGE_FEATURE_FLAGS: Final[dict[str, bool]] = {
    "enable_portfolio_margins": True,
    "enable_mmp": True,
    "enable_batch_orders": True,
    "enable_options_trading": True,
    "enable_mark_price_feeds": True,
    "enable_funding_rate_feeds": True,
    "enable_position_updates": True,
    "enable_margin_updates": True,
}


# -- LOGGING AND MONITORING CONSTANTS --------------------------------------------------------

# Logger names
DELTA_EXCHANGE_LOGGER_NAMES: Final[dict[str, str]] = {
    "adapter": "nautilus_trader.adapters.delta_exchange",
    "data": "nautilus_trader.adapters.delta_exchange.data",
    "execution": "nautilus_trader.adapters.delta_exchange.execution",
    "providers": "nautilus_trader.adapters.delta_exchange.providers",
    "factories": "nautilus_trader.adapters.delta_exchange.factories",
    "http": "nautilus_trader.adapters.delta_exchange.http",
    "websocket": "nautilus_trader.adapters.delta_exchange.websocket",
}

# Performance monitoring constants
DELTA_EXCHANGE_PERFORMANCE_METRICS: Final[list[str]] = [
    "http_request_count",
    "http_request_duration",
    "http_error_count",
    "ws_connection_count",
    "ws_message_count",
    "ws_error_count",
    "order_submission_latency",
    "order_update_latency",
    "market_data_latency",
    "cache_hit_ratio",
]

# Health check constants
DELTA_EXCHANGE_HEALTH_CHECK_ENDPOINTS: Final[list[str]] = [
    "/v2/products",
    "/v2/assets",
]

DELTA_EXCHANGE_HEALTH_CHECK_TIMEOUT: Final[int] = 5  # seconds


# -- VALIDATION CONSTANTS --------------------------------------------------------------------

# API key validation patterns
DELTA_EXCHANGE_API_KEY_PATTERN: Final[str] = r"^[a-zA-Z0-9]{32,64}$"
DELTA_EXCHANGE_API_SECRET_PATTERN: Final[str] = r"^[a-zA-Z0-9+/=]{40,128}$"

# Symbol validation patterns
DELTA_EXCHANGE_SYMBOL_PATTERN: Final[str] = r"^[A-Z0-9_-]+$"
DELTA_EXCHANGE_PRODUCT_ID_PATTERN: Final[str] = r"^\d+$"

# Order validation limits
DELTA_EXCHANGE_MAX_ORDER_PRICE: Final[str] = "1000000000.0"  # Maximum order price
DELTA_EXCHANGE_MIN_ORDER_PRICE: Final[str] = "0.000001"     # Minimum order price
DELTA_EXCHANGE_MAX_ORDERS_PER_BATCH: Final[int] = 20        # Maximum orders per batch request

# Position validation limits
DELTA_EXCHANGE_MAX_POSITION_SIZE: Final[str] = "1000000.0"  # Maximum position size
DELTA_EXCHANGE_MIN_POSITION_SIZE: Final[str] = "0.000001"   # Minimum position size


# -- BACKWARD COMPATIBILITY CONSTANTS --------------------------------------------------------

# Legacy constants for backward compatibility (deprecated - use new constants above)
DELTA_EXCHANGE = DELTA_EXCHANGE_VENUE  # Legacy venue constant
DELTA_EXCHANGE_PRODUCT_TYPES: Final[list[str]] = [e.value for e in DeltaExchangeProductType]
DELTA_EXCHANGE_ORDER_TYPES: Final[list[str]] = [e.value for e in DeltaExchangeOrderType]
DELTA_EXCHANGE_ORDER_STATES: Final[list[str]] = [e.value for e in DeltaExchangeOrderStatus]
DELTA_EXCHANGE_TIME_IN_FORCE: Final[list[str]] = [e.value for e in DeltaExchangeTimeInForce]


# -- COMPREHENSIVE CONSTANT COLLECTIONS ------------------------------------------------------

# All Delta Exchange constants for validation and testing
DELTA_EXCHANGE_ALL_CONSTANTS: Final[dict[str, any]] = {
    # Venue and client identifiers
    "venue": DELTA_EXCHANGE_VENUE,
    "client_id": DELTA_EXCHANGE_CLIENT_ID,

    # API URLs
    "http_urls": DELTA_EXCHANGE_HTTP_URLS,
    "ws_urls": DELTA_EXCHANGE_WS_URLS,

    # WebSocket channels
    "public_channels": DELTA_EXCHANGE_WS_PUBLIC_CHANNELS,
    "private_channels": DELTA_EXCHANGE_WS_PRIVATE_CHANNELS,
    "all_channels": DELTA_EXCHANGE_WS_ALL_CHANNELS,

    # Enumerations
    "product_types": [e.value for e in DeltaExchangeProductType],
    "order_types": [e.value for e in DeltaExchangeOrderType],
    "order_statuses": [e.value for e in DeltaExchangeOrderStatus],
    "time_in_force": [e.value for e in DeltaExchangeTimeInForce],
    "order_sides": [e.value for e in DeltaExchangeOrderSide],
    "trading_statuses": [e.value for e in DeltaExchangeTradingStatus],

    # Supported Nautilus types
    "supported_order_types": DELTA_EXCHANGE_SUPPORTED_ORDER_TYPES,
    "supported_time_in_force": DELTA_EXCHANGE_SUPPORTED_TIME_IN_FORCE,
    "supported_order_sides": DELTA_EXCHANGE_SUPPORTED_ORDER_SIDES,

    # Data model mappings
    "order_type_mappings": {
        "to_nautilus": DELTA_EXCHANGE_TO_NAUTILUS_ORDER_TYPE,
        "from_nautilus": NAUTILUS_TO_DELTA_EXCHANGE_ORDER_TYPE,
    },
    "order_status_mappings": {
        "to_nautilus": DELTA_EXCHANGE_TO_NAUTILUS_ORDER_STATUS,
    },
    "time_in_force_mappings": {
        "to_nautilus": DELTA_EXCHANGE_TO_NAUTILUS_TIME_IN_FORCE,
        "from_nautilus": NAUTILUS_TO_DELTA_EXCHANGE_TIME_IN_FORCE,
    },
    "order_side_mappings": {
        "to_nautilus": DELTA_EXCHANGE_TO_NAUTILUS_ORDER_SIDE,
        "from_nautilus": NAUTILUS_TO_DELTA_EXCHANGE_ORDER_SIDE,
    },

    # Configuration and limits
    "default_config": DELTA_EXCHANGE_DEFAULT_CONFIG,
    "feature_flags": DELTA_EXCHANGE_FEATURE_FLAGS,
    "error_codes": DELTA_EXCHANGE_ERROR_CODES,
    "environments": DELTA_EXCHANGE_ENVIRONMENTS,

    # Performance and monitoring
    "performance_metrics": DELTA_EXCHANGE_PERFORMANCE_METRICS,
    "logger_names": DELTA_EXCHANGE_LOGGER_NAMES,
}
