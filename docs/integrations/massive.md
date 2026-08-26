# Massive

Massive (formerly Polygon.io) is a US market data provider offering real-time
and historical data for stocks, options, indices, forex, and crypto through
REST and WebSocket APIs. This adapter supports live market data ingest and
historical data requests for **US equities** (stocks). It is a data-only
integration; there is no execution support.

## Overview

The Massive adapter is implemented in Rust and exposed to Python through
configurations, factories, enums, and constants.

Components:

- `MassiveRawHttpClient`: Low-level REST client owning transport, bearer authentication, and rate limits.
- `MassiveHttpClient`: Domain REST client parsing venue responses into Nautilus types.
- `MassiveWebSocketClient`: Low-level WebSocket connectivity with API-key authentication.
- `MassiveInstrumentProvider`: Instrument parsing and loading from the reference tickers endpoint.
- `MassiveDataClient`: Market data feed manager.
- `MassiveDataClientFactory`: Data client factory.

Python surface available from `nautilus_trader.adapters.massive`:

- `MassiveDataClientConfig`
- `MassiveDataClientFactory`
- `MassiveDataFeed`
- `MASSIVE`, `MASSIVE_CLIENT_ID`, and `MASSIVE_VENUE`

## Massive documentation

Massive provides API documentation for its REST and WebSocket endpoints:

- [Stocks REST API](https://massive.com/docs/rest/stocks/overview)
- [Stocks WebSocket API](https://massive.com/docs/websocket/stocks/overview)

It's recommended you also refer to the Massive documentation in conjunction
with this NautilusTrader integration guide.

## Instruments

Instruments are sourced from the reference tickers endpoint for the US stocks
market and mapped to `Equity` instruments with venue `MASSIVE`, e.g.
`AAPL.MASSIVE`. Tickers containing class suffixes (such as `BRK.A`) are kept
verbatim in the symbol.

By default the provider loads **all active US stock tickers** (several
thousand instruments) on connect. Provide `symbols` in the config to restrict
loading to a specific set:

```python
config = MassiveDataClientConfig(symbols=["AAPL", "MSFT", "SPY"])
```

## Live data feeds

The client subscribes to the Massive US stocks WebSocket cluster and streams:

| Nautilus subscription      | Massive channel | Notes                              |
|:---------------------------|:----------------|:-----------------------------------|
| `subscribe_trade_ticks`    | `T.<ticker>`    | Tick-level trades.                 |
| `subscribe_quote_ticks`    | `Q.<ticker>`    | NBBO quotes; sizes are in shares.  |
| `subscribe_bars` (1-SECOND)| `A.<ticker>`    | Per-second aggregates.             |
| `subscribe_bars` (1-MINUTE)| `AM.<ticker>`   | Per-minute aggregates.             |

Only externally aggregated 1-SECOND and 1-MINUTE `LAST` price bars are
streamed by the venue. Other bar specifications can be aggregated internally
by Nautilus from trades or quotes.

The `feed` config selects the real-time (`wss://socket.massive.com`) or
15-minute delayed (`wss://delayed.massive.com`) cluster; which feeds a key can
access depends on the subscribed Massive plan.

## Historical data

Historical requests are served through the REST API:

| Nautilus request      | Massive endpoint                       | Notes                                        |
|:----------------------|:---------------------------------------|:---------------------------------------------|
| `request_instruments` | `/v3/reference/tickers`                | Active US stock tickers.                     |
| `request_bars`        | `/v2/aggs/ticker/.../range/...`        | Second through month windows; adjusted by default. |
| `request_trades`      | `/v3/trades/{ticker}`                  | Tick-level trade history.                    |
| `request_quotes`      | `/v3/quotes/{ticker}`                  | NBBO quote history.                          |

Aggregate bars are timestamped on the close of the window by Nautilus
convention; set `bars_timestamp_on_close=False` to timestamp on the open.
Split- and dividend-adjusted prices are requested by default; set
`adjusted_bars=False` for unadjusted prices.

## Configuration

The API key can be provided via `MassiveDataClientConfig(api_key=...)` or the
`MASSIVE_API_KEY` environment variable (recommended).

### Configuration options

| Option                    | Default      | Description                                                       |
|:--------------------------|:-------------|:------------------------------------------------------------------|
| `api_key`                 | `None`       | API key (falls back to `MASSIVE_API_KEY` env var).                |
| `base_url_rest`           | `None`       | Override for the REST base URL.                                   |
| `base_url_ws`             | `None`       | Override for the WebSocket URL.                                   |
| `feed`                    | `REAL_TIME`  | Market data feed cluster (`REAL_TIME` or `DELAYED`).              |
| `symbols`                 | `[]`         | Tickers to load on connect; empty loads all active US stocks.     |
| `http_timeout_secs`       | `60`         | HTTP request timeout.                                             |
| `adjusted_bars`           | `True`       | Request split/dividend adjusted aggregate bars.                   |
| `bars_timestamp_on_close` | `True`       | Timestamp bars on window close (Nautilus convention).             |
| `transport_backend`       | `TUNGSTENITE`| WebSocket transport backend.                                      |

An example configuration:

```python
from nautilus_trader.adapters.massive import MassiveDataClientConfig
from nautilus_trader.adapters.massive import MassiveDataFeed

data_config = MassiveDataClientConfig(
    api_key=None,  # Will use the MASSIVE_API_KEY env var
    feed=MassiveDataFeed.REAL_TIME,
    symbols=["AAPL", "MSFT"],
)
```

Pair this config with `MassiveDataClientFactory` when registering the data
client in `LiveNode.builder(...)`, as demonstrated in the
[Rust example](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/massive/examples/).
