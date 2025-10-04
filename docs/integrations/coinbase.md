# Coinbase Advanced Trade (US)

NautilusTrader supports live trading with [Coinbase Advanced Trade API](https://docs.cloud.coinbase.com/advanced-trade-api/docs/welcome).

⚠️ **Note**: This adapter is for **Coinbase Advanced Trade** (available in the US and select regions). This is different from:
- Coinbase Pro (deprecated)
- Coinbase International Exchange
- Coinbase Prime

## Overview

The Coinbase Advanced Trade adapter provides:
- ✅ Real-time market data via WebSocket
- ✅ Order execution (market and limit orders)
- ✅ Account balance tracking
- ✅ Position management
- ✅ Historical data access
- ✅ Instrument provider with automatic updates

## Features

### Supported Order Types
- Market orders
- Limit orders (Good-Till-Cancelled, Good-Till-Date, Immediate-Or-Cancel, Fill-Or-Kill)
- Stop-loss orders
- Stop-limit orders

### Market Data
- Real-time ticker updates
- Level 2 order book data
- Trade execution data
- Candles/OHLCV data
- 24-hour statistics

### Account Management
- Real-time balance updates
- Position tracking
- Order status updates
- Fill notifications

## Authentication

Coinbase Advanced Trade API uses JWT (JSON Web Token) authentication with ES256 (ECDSA with P-256 curve and SHA-256).

### Creating API Credentials

1. Log in to [Coinbase](https://www.coinbase.com/) (US account)
2. Navigate to **Settings** → **API**
3. Click **New API Key**
4. Select **Advanced Trade** as the API type (NOT Coinbase Pro)
5. Choose permissions:
   - **View**: Required for market data and account information
   - **Trade**: Required for order execution
6. Save your API key name and private key

⚠️ **Important**: Make sure you're creating an **Advanced Trade** API key, not a legacy Coinbase Pro key.

Your API credentials will look like this (example - NOT REAL):
```
API Key: organizations/00000000-0000-0000-0000-000000000000/apiKeys/11111111-1111-1111-1111-111111111111
Private Key: -----BEGIN EC PRIVATE KEY-----
MHcCAQEEIEXAMPLE1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567oAoGCCqGSM49
AwEHoUQDQgAEEXAMPLEKEY1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ1234567890AB
CDEFGHIJKLMNOPQRSTUVWXYZ1234567890ABCDEFGHIJKLMNOPQRSTUVWXYZ==
-----END EC PRIVATE KEY-----
```

⚠️ **Important**:
- The above is an EXAMPLE ONLY and will not work
- Save your actual private key securely - Coinbase will only show it once
- Never commit your real API credentials to version control

### Setting Environment Variables

Set your API credentials as environment variables:

```bash
export COINBASE_API_KEY="organizations/your-org-id/apiKeys/your-key-id"
export COINBASE_API_SECRET="-----BEGIN EC PRIVATE KEY-----
MHcCAQEEI...
-----END EC PRIVATE KEY-----"
```

## Installation

The Coinbase adapter is included with NautilusTrader. No additional installation is required.

```bash
pip install nautilus_trader
```

## Configuration

### Data Client Configuration

```python
from nautilus_trader.adapters.coinbase.config import CoinbaseDataClientConfig

config = CoinbaseDataClientConfig(
    api_key=None,  # Will use COINBASE_API_KEY env var
    api_secret=None,  # Will use COINBASE_API_SECRET env var
    base_url_http=None,  # Optional: custom HTTP base URL
    base_url_ws=None,  # Optional: custom WebSocket base URL
    http_timeout_secs=60,  # HTTP request timeout
    update_instruments_interval_mins=None,  # Auto-update instruments interval
)
```

### Execution Client Configuration

```python
from nautilus_trader.adapters.coinbase.config import CoinbaseExecClientConfig

config = CoinbaseExecClientConfig(
    api_key=None,  # Will use COINBASE_API_KEY env var
    api_secret=None,  # Will use COINBASE_API_SECRET env var
    base_url_http=None,  # Optional: custom HTTP base URL
    base_url_ws=None,  # Optional: custom WebSocket base URL
    http_timeout_secs=60,  # HTTP request timeout
    update_instruments_interval_mins=None,  # Auto-update instruments interval
)
```

### Trading Node Configuration

```python
from nautilus_trader.config import TradingNodeConfig
from nautilus_trader.adapters.coinbase.config import (
    CoinbaseDataClientConfig,
    CoinbaseExecClientConfig,
)

config = TradingNodeConfig(
    data_clients={
        "COINBASE": CoinbaseDataClientConfig(),
    },
    exec_clients={
        "COINBASE": CoinbaseExecClientConfig(),
    },
)
```

## Examples

### Basic Connection Test

```python
import asyncio
import os
from nautilus_trader.adapters.coinbase.factories import get_coinbase_http_client

async def main():
    client = get_coinbase_http_client(
        api_key=os.getenv("COINBASE_API_KEY"),
        api_secret=os.getenv("COINBASE_API_SECRET"),
    )
    
    # Fetch products
    products = await client.list_products()
    print(f"Found {len(products.get('products', []))} products")
    
    # Fetch accounts
    accounts = await client.list_accounts()
    print(f"Found {len(accounts.get('accounts', []))} accounts")

asyncio.run(main())
```

### Stream Market Data

```python
import asyncio
import os
from nautilus_trader.adapters.coinbase.factories import get_coinbase_websocket_client

async def handle_message(message: dict):
    print(f"Received: {message}")

async def main():
    client = get_coinbase_websocket_client(
        api_key=os.getenv("COINBASE_API_KEY"),
        api_secret=os.getenv("COINBASE_API_SECRET"),
        message_handler=handle_message,
    )
    
    await client.connect()
    await client.subscribe_ticker(["BTC-USD", "ETH-USD"])
    
    # Keep connection alive
    while True:
        await asyncio.sleep(1)

asyncio.run(main())
```

### Live Trading Strategy

See `examples/live/coinbase/simple_strategy.py` for a complete example.

## API Limitations

### Rate Limits

Coinbase Advanced Trade API has the following rate limits:
- **Public endpoints**: 10 requests per second
- **Private endpoints**: 15 requests per second

The adapter automatically handles rate limiting.

### Order Size Limits

Each trading pair has minimum and maximum order sizes. Check the product details:

```python
product = await client.get_product("BTC-USD")
base_min_size = product.get('base_min_size')
base_max_size = product.get('base_max_size')
```

### Fees

Coinbase Advanced Trade uses a maker-taker fee model:
- **Taker fees**: 0.40% - 1.20% (market orders)
- **Maker fees**: 0.00% - 0.60% (limit orders)

Fees depend on your 30-day trading volume. See [Coinbase fee structure](https://help.coinbase.com/en/advanced-trade/trading-and-funding/trading-fees) for details.

## Troubleshooting

### 401 Unauthorized

**Cause**: Invalid API credentials or JWT signature.

**Solutions**:
1. Verify your API key and secret are correct
2. Ensure your API key has the required permissions (View + Trade)
3. Check that your API key is not expired
4. Verify you're using Coinbase Advanced Trade API credentials (not Coinbase Pro)

### Connection Timeout

**Cause**: Network issues or API downtime.

**Solutions**:
1. Check your internet connection
2. Verify Coinbase API status at [status.coinbase.com](https://status.coinbase.com/)
3. Increase `http_timeout_secs` in configuration

### Order Rejected

**Cause**: Insufficient funds, invalid order size, or market conditions.

**Solutions**:
1. Check your account balance
2. Verify order size meets minimum/maximum requirements
3. Ensure the trading pair is active and not in post-only mode

## Support

For issues specific to the Coinbase adapter:
- GitHub Issues: [nautechsystems/nautilus_trader](https://github.com/nautechsystems/nautilus_trader/issues)
- Discord: [NautilusTrader Discord](https://discord.gg/AUWFbZeXbF)

For Coinbase API issues:
- Coinbase Support: [help.coinbase.com](https://help.coinbase.com/)
- API Documentation: [docs.cloud.coinbase.com](https://docs.cloud.coinbase.com/advanced-trade-api/docs/welcome)

## References

- [Coinbase Advanced Trade API Documentation](https://docs.cloud.coinbase.com/advanced-trade-api/docs/welcome)
- [Coinbase API Authentication](https://docs.cloud.coinbase.com/advanced-trade-api/docs/rest-api-auth)
- [Coinbase WebSocket API](https://docs.cloud.coinbase.com/advanced-trade-api/docs/ws-overview)
- [Coinbase Fee Structure](https://help.coinbase.com/en/advanced-trade/trading-and-funding/trading-fees)

