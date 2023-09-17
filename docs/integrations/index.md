# Integrations

NautilusTrader is designed in a modular way to work with 'adapters' which provide
connectivity to data publishers and/or trading venues - converting their raw API
into a unified interface. The following integrations are currently supported:

```{warning}
The initial integrations for the project are currently under heavy construction. 
It's advised to conduct some of your own testing with small amounts of capital before
running strategies which are able to access larger capital allocations.
```

| Name                                                      | ID        | Type                    | Status                                                  | Docs                                                              |
| :-------------------------------------------------------- | :-------- | :---------------------- | :------------------------------------------------------ | :---------------------------------------------------------------- |
| [Betfair](https://betfair.com)                            | `BETFAIR` | Sports Betting Exchange | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/betfair.html) |
| [Binance](https://binance.com)                            | `BINANCE` | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
| [Binance US](https://binance.us)                          | `BINANCE` | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
| [Binance Futures](https://www.binance.com/en/futures)     | `BINANCE` | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/stable-green)    | [Guide](https://docs.nautilustrader.io/integrations/binance.html) |
| [Bybit](https://www.bybit.com)                            | `BYBIT`   | Crypto Exchange (CEX)   | ![status](https://img.shields.io/badge/building-orange) |                                                                   |
| [Interactive Brokers](https://www.interactivebrokers.com) | `IB`      | Brokerage (multi-venue) | ![status](https://img.shields.io/badge/beta-yellow)     | [Guide](https://docs.nautilustrader.io/integrations/ib.html)      |

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

- All symbols will match the native/local symbol for the exchange, unless there are conflicts (such as Binance using the same symbol for both Spot and Perpetual Futures markets).
- All timestamps will be either normalized to UNIX nanoseconds, or clearly marked as UNIX milliseconds by appending `_ms` to param and property names.

```{eval-rst}
.. toctree::
   :maxdepth: 2
   :glob:
   :titlesonly:
   :hidden:
   
   betfair.md
   binance.md
   ib.md

```
