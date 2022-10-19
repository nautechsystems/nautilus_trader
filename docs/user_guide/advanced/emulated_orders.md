# Emulated Orders

The platform makes it possible to emulate any order type locally, regardless
of whether it is supported on a trading venue. The logic and code paths for 
order emulation are exactly the same for both backtesting and live trading, and
utilize a common `OrderEmulator` component.

## Life cycle
Once an emulated order is triggered or matched locally based on data feeds, then 
a 'real' order will be submitted to the venue.

The following table lists which order types are possible to emulate, and
which order type they transform to when being released for submission to the 
trading venue.

Emulated orders which have been released will always be one of the following types:
- `MARKET`
- `LIMIT`

## Order types
|                        | Can emulate | Released type |
|------------------------|-------------|---------------|
| `MARKET`               | No          | -             |
| `MARKET_TO_LIMIT`      | No          | -             |
| `LIMIT`                | Yes         | `MARKET`      |
| `STOP_MARKET`          | Yes         | `MARKET`      |
| `STOP_LIMIT`           | Yes         | `LIMIT`       |
| `MARKET_IF_TOUCHED`    | Yes         | `MARKET`      |
| `LIMIT_IF_TOUCHED`     | Yes         | `LIMIT`       |
| `TRAILING_STOP_MARKET` | Yes         | `MARKET`      |
| `TRAILING_STOP_LIMIT`  | Yes         | `LIMIT`       |
