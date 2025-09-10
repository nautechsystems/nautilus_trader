# nautilus-delta-exchange

Delta Exchange adapter for Nautilus Trader.

This adapter provides integration with Delta Exchange, a derivatives trading platform
that offers perpetual futures and options trading.

## Features

- REST API integration for account management and order execution
- WebSocket integration for real-time market data and order updates
- Support for perpetual futures and options trading
- HMAC-SHA256 authentication
- Comprehensive error handling and rate limiting

## Configuration

The adapter supports both production and testnet environments:

- **Production**: `https://api.delta.exchange`
- **Testnet**: `https://testnet-api.delta.exchange`

## Authentication

Delta Exchange uses HMAC-SHA256 signature authentication. You'll need:

- API Key
- API Secret

## Supported Instruments

- Perpetual Futures (e.g., BTCUSD, ETHUSD)
- Call Options (e.g., C-BTC-90000-310125)
- Put Options (e.g., P-BTC-38100-230124)

## Rate Limits

- REST API: Up to 100 requests per second
- WebSocket: Up to 150 connections per IP per 5 minutes

## License

Licensed under the GNU Lesser General Public License Version 3.0.
