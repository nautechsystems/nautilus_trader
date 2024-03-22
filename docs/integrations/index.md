# Integrations

```{eval-rst}
.. toctree::
   :maxdepth: 2
   :glob:
   :titlesonly:
   :hidden:
   
   betfair.md
   binance.md
   databento.md
   ib.md
```

NautilusTrader is designed in a modular way to work with *adapters* which provide
connectivity to trading venues and data providers - converting their raw API
into a unified interface. The following integrations are currently supported:

| Name                                                      | ID                    | Type                    | Status                                                  | Docs                                                                |
| :-------------------------------------------------------- | :-------------------- | :---------------------- | :------------------------------------------------------ | :------------------------------------------------------------------ |
| [Betfair](https://betfair.com)                            | `BETFAIR`             | Sports Betting Exchange | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/betfair.html)   |
| [Binance](https://binance.com)                            | `BINANCE`             | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html)   |
| [Binance US](https://binance.us)                          | `BINANCE`             | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html)   |
| [Binance Futures](https://www.binance.com/en/futures)     | `BINANCE`             | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html)   |
| [Bybit](https://www.bybit.com)                            | `BYBIT`               | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/building-orange) |                                                                     |
| [Databento](https://databento.com)                        | `DATABENTO`           | Data Provider           | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/databento.html) |
| [Interactive Brokers](https://www.interactivebrokers.com) | `INTERACTIVE_BROKERS` | Brokerage (multi-venue) | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/ib.html)        |

- `ID:` The default client ID for the integrations adapter clients
- `Type:` The type of integration (often the venue type)

### Status
- `building` - Under construction and likely not in a usable state
- `beta` - Completed to a minimally working state and in a 'beta' testing phase
- `stable` - Stabilized feature set and API, the integration has been tested by both developers and users to a reasonable level (some bugs may still remain)

## Implementation goals

The primary goal of NautilusTrader is to provide a unified trading system for 
use with a variety of integrations. To support the widest range of trading 
strategies, priority will be given to 'standard' functionality:

- Requesting historical market data
- Streaming live market data
- Reconciling execution state
- Submitting standard order types with standard execution instructions
- Modifying existing orders (if possible on an exchange)
- Canceling orders

The implementation of each integration aims to meet the following criteria:

- Low-level client components should match the exchange API as closely as possible
- The full range of an exchanges functionality (where applicable to NautilusTrader), should _eventually_ be supported
- Exchange specific data types will be added to support the functionality and return
  types which are reasonably expected by a user
- Actions which are unsupported by either the exchange or NautilusTrader, will be explicitly logged as
a warning or error when a user attempts to perform said action

## API unification
All integrations must be compatible with the NautilusTrader API at the system boundary,
this means there is some normalization and standardization needed.

- All symbols will match the raw/native/local symbol for the exchange, unless there are conflicts (such as Binance using the same symbol for both Spot and Perpetual Futures markets)
- All timestamps will be either normalized to UNIX nanoseconds, or clearly marked as UNIX milliseconds by appending `_ms` to param and property names
