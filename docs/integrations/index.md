# Integrations

NautilusTrader uses modular *adapters* to connect to trading venues and data providers, translating raw APIs into a unified interface and normalized domain model.

The following integrations are currently supported:

| Name                                                                         | ID                    | Type                    | Status                                                  | Docs                                   |
| :--------------------------------------------------------------------------- | :-------------------- | :---------------------- | :------------------------------------------------------ | :------------------------------------- |
| [Betfair](https://betfair.com)                                               | `BETFAIR`             | Sports Betting Exchange | ![status](https://img.shields.io/badge/stable-green)    | [Guide](betfair.md)       |
| [Binance](https://binance.com)                                               | `BINANCE`             | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](binance.md)       |
| [BitMEX](https://www.bitmex.com)                                             | `BITMEX`              | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](bitmex.md)        |
| [Bybit](https://www.bybit.com)                                               | `BYBIT`               | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](bybit.md)         |
| [Coinbase International](https://www.coinbase.com/en/international-exchange) | `COINBASE_INTX`       | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](coinbase_intx.md) |
| [Databento](https://databento.com)                                           | `DATABENTO`           | Data Provider           | ![status](https://img.shields.io/badge/stable-green)    | [Guide](databento.md)     |
| [dYdX](https://dydx.exchange/)                                               | `DYDX`                | Crypto Exchange (DEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](dydx.md)          |
| [Hyperliquid](https://hyperliquid.xyz)                                       | `HYPERLIQUID`         | Crypto Exchange (DEX)   | ![status](https://img.shields.io/badge/building-orange) | [Guide](hyperliquid.md)   |
| [Interactive Brokers](https://www.interactivebrokers.com)                    | `INTERACTIVE_BROKERS` | Brokerage (multi-venue) | ![status](https://img.shields.io/badge/stable-green)    | [Guide](ib.md)            |
| [OKX](https://okx.com)                                                       | `OKX`                 | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](okx.md)           |
| [Polymarket](https://polymarket.com)                                         | `POLYMARKET`          | Prediction Market (DEX) | ![status](https://img.shields.io/badge/stable-green)    | [Guide](polymarket.md)    |
| [Tardis](https://tardis.dev)                                                 | `TARDIS`              | Crypto Data Provider    | ![status](https://img.shields.io/badge/stable-green)    | [Guide](tardis.md)        |

- **ID**: The default client ID for the integrations adapter clients.
- **Type**: The type of integration (often the venue type).

## Status

- `building`: Under construction and likely not in a usable state.
- `beta`: Completed to a minimally working state and in a 'beta' testing phase.
- `stable`: Stabilized feature set and API, the integration has been tested by both developers and users to a reasonable level (some bugs may still remain).

## Implementation goals

The primary goal of NautilusTrader is to provide a unified trading system for
use with a variety of integrations. To support the widest range of trading
strategies, priority will be given to *standard* functionality:

- Requesting historical market data.
- Streaming live market data.
- Reconciling execution state.
- Submitting standard order types with standard execution instructions.
- Modifying existing orders (if possible on an exchange).
- Canceling orders.

The implementation of each integration aims to meet the following criteria:

- Low-level client components should match the exchange API as closely as possible.
- The full range of an exchange's functionality (where applicable to NautilusTrader) should *eventually* be supported.
- Exchange specific data types will be added to support the functionality and return types which are reasonably expected by a user.
- Actions unsupported by an exchange or NautilusTrader will be logged as a warning or error when invoked.

## API unification

All integrations must conform to NautilusTrader’s system API, requiring normalization and standardization:

- Symbols should use the venue’s native symbol format unless disambiguation is required (e.g., Binance Spot vs. Binance Futures).
- Timestamps must use UNIX epoch nanoseconds. If milliseconds are used, field/property names should explicitly end with `_ms`.
