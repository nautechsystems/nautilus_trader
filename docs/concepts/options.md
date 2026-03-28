# Options

Nautilus provides first-class support for options trading across traditional
and crypto markets. This includes option-specific instrument types, venue-provided
Greeks streaming, option chain aggregation, and a local Black-Scholes Greeks calculator
for risk management.

## Option instrument types

The platform defines several option instrument types:

| Instrument       | Description                                                                            |
|------------------|----------------------------------------------------------------------------------------|
| `OptionContract` | Exchange‑traded option (put or call) on an underlying with strike and expiry.           |
| `OptionSpread`   | Exchange‑defined multi‑leg options strategy (vertical, calendar, straddle) as one line. |
| `CryptoOption`   | Option on a crypto underlying with crypto quote/settlement; inverse or quanto styles.   |
| `BinaryOption`   | Fixed‑payout option that settles to 0 or 1 based on a binary outcome.                  |

Greeks-relevant metadata varies by instrument type:

- `OptionContract`, `CryptoOption`: full Greeks inputs including `strike_price`,
  `option_kind` (CALL/PUT), `expiration_utc`, `underlying`, `multiplier`.
- `OptionSpread`: a combination of up to 4 option legs, each weighted by a
  ratio. Has `underlying`, `expiration_utc`, and `strategy_type` (vertical,
  calendar, straddle, etc.). Per-leg `strike_price` and `option_kind` live on
  each leg's `OptionContract`, not on the spread itself. Greeks are computed
  per leg and aggregated. Spreads are commonly used for orders (the exchange
  executes as a single order), while the individual legs appear as positions.
- `BinaryOption`: has `expiration_utc` and `outcome`/`description`, but no
  `strike_price`, `option_kind`, or `underlying`.

## Subscribing to Greeks

Venues like Deribit, Bybit, and OKX publish real-time Greeks alongside their options markets.
Nautilus provides two subscription levels:

- **Per-instrument Greeks**: subscribe to individual option contracts.
- **Option chain slices**: subscribe to an aggregated view of an entire option series.

### Per-instrument Greeks

Subscribe to venue-provided Greeks for a single option contract from an actor or strategy:

```python
from nautilus_trader.model.identifiers import ClientId

client_id = ClientId("DERIBIT")
self.subscribe_option_greeks(instrument_id, client_id=client_id)
```

Handle incoming updates by implementing the `on_option_greeks` handler:

```python
def on_option_greeks(self, greeks) -> None:
    self.log.info(
        f"{greeks.instrument_id}: "
        f"delta={greeks.delta:.4f} gamma={greeks.gamma:.6f} "
        f"vega={greeks.vega:.4f} theta={greeks.theta:.4f} "
        f"mark_iv={greeks.mark_iv} underlying={greeks.underlying_price}"
    )
```

To stop receiving updates:

```python
self.unsubscribe_option_greeks(instrument_id, client_id=client_id)
```

### Option chain subscriptions

An option chain subscription aggregates quotes and Greeks across all strikes in an
option series into periodic `OptionChainSlice` snapshots. The `DataEngine` creates
one `OptionChainManager` per series and owns the full lifecycle: routing incoming
data through the manager, publishing snapshots, and managing wire subscriptions.

```python
from nautilus_trader.core import nautilus_pyo3

series_id = nautilus_pyo3.OptionSeriesId(...)  # identifies the series (venue, underlying, expiry)

# Subscribe to 5 strikes above and below ATM, snapshot every 1000ms
strike_range = nautilus_pyo3.StrikeRange.atm_relative(strikes_above=5, strikes_below=5)
self.subscribe_option_chain(
    series_id,
    strike_range=strike_range,
    snapshot_interval_ms=1000,
)
```

Handle snapshots by implementing the `on_option_chain` handler:

```python
def on_option_chain(self, chain) -> None:
    for strike in chain.strikes():
        call = chain.get_call(strike)
        put = chain.get_put(strike)
        if call and call.greeks:
            self.log.info(f"Call {strike}: delta={call.greeks.delta:.4f}")
```

### Strike range filtering

`StrikeRange` controls which strikes are active in a chain subscription:

| Variant        | Description                                          | Example                                       |
|----------------|------------------------------------------------------|-----------------------------------------------|
| `Fixed`        | Subscribe to an explicit set of strikes.             | `nautilus_pyo3.StrikeRange.fixed([...])`       |
| `AtmRelative`  | N strikes above and N below the current ATM strike.  | `nautilus_pyo3.StrikeRange.atm_relative(5, 5)` |
| `AtmPercent`   | All strikes within a percentage band around ATM.     | `nautilus_pyo3.StrikeRange.atm_percent(0.10)`  |

For ATM-based variants, subscriptions are deferred until the ATM price is determined.
ATM is derived from the forward price embedded in venue-provided `OptionGreeks` updates
(the `underlying_price` field). It can also be seeded from an initial forward price
fetched via HTTP, allowing instant bootstrap before live WebSocket ticks arrive. As ATM
shifts, the active strike set rebalances automatically.

### Snapshot vs. raw mode

The `snapshot_interval_ms` parameter controls publishing behavior:

- **Snapshot mode** (`snapshot_interval_ms=1000`): Quotes and Greeks accumulate in a
  buffer and publish as an `OptionChainSlice` on a timer. Suitable for periodic
  portfolio rebalancing or UI display.
- **Raw mode** (`snapshot_interval_ms=None`): Each quote or Greeks update publishes
  a slice immediately. Suitable for latency-sensitive strategies that react to
  individual updates.

## Option chain architecture

The option chain system is event-driven and built around per-series isolation. The
`DataEngine` creates one `OptionChainManager` (a PyO3 wrapper around the Rust
`OptionChainAggregator` and `AtmTracker`) per subscribed option series. The engine
owns the lifecycle: subscription routing, timer management, and message bus publishing.
The manager handles only aggregation state and ATM tracking.

```mermaid
flowchart TD
    subgraph DataEngine
        DE[DataEngine]
        TMR[SnapshotTimer]
    end

    subgraph "OptionChainManager (per series)"
        MGR[Manager / PyO3]
        AGG[OptionChainAggregator]
        ATM[AtmTracker]
    end

    DC[DataClient] -- QuoteTick --> DE
    DC -- OptionGreeks --> DE
    DE -- "handle_quote()" --> MGR
    DE -- "handle_greeks()" --> MGR
    MGR --> AGG
    MGR --> ATM
    ATM -- "forward price" --> AGG
    TMR -- "timer tick" --> DE
    DE -- "snapshot()" --> MGR
    MGR -- "OptionChainSlice" --> DE
    DE -- publish --> MB((MessageBus))
    MB -- "on_option_chain" --> S[Actor / Strategy]
    DE -- "sub/unsub" --> DC
```

### Component responsibilities

#### DataEngine

Holds one `OptionChainManager` per active `OptionSeriesId`. On
`SubscribeOptionChain`, it resolves instruments from the cache, creates the
manager, subscribes active instruments to the data client, and sets up the
snapshot timer. On each timer tick, it calls `manager.check_rebalance()` and
`manager.snapshot()`, forwarding any subscription changes directly to the data
client. On `UnsubscribeOptionChain` or when all instruments expire, it tears
down the manager, cancels the timer, and unsubscribes wire-level feeds.

#### OptionChainManager (PyO3)

A thin PyO3 wrapper around `OptionChainAggregator` and `AtmTracker`. It does
not interact with the message bus, clock, or data clients. The `DataEngine`
feeds it market data through `handle_quote()` and `handle_greeks()`, and
retrieves snapshots via `snapshot()`. Both `handle_*` methods return a boolean
indicating whether ATM bootstrap occurred (first ATM price arrived), which the
engine uses to trigger subscription of the real active instrument set.

#### OptionChainAggregator

Accumulates quotes and Greeks into call/put buffers using keep-latest semantics.
Instruments that did not update since the last snapshot are still included. Greeks
that arrive before any quote for an instrument are held in a `pending_greeks`
buffer and attached when the first quote arrives. On each `snapshot()` call, the
aggregator produces an immutable `OptionChainSlice`.

#### AtmTracker

Derives the ATM price reactively from the `underlying_price` field in incoming
`OptionGreeks` events (the venue-provided forward price for that expiry). It can
be pre-seeded from an HTTP forward price response for instant bootstrap without
waiting for WebSocket ticks.

### Bootstrap and rebalancing

For ATM-based strike ranges (`AtmRelative`, `AtmPercent`), the active instrument
set cannot be determined until the ATM price is known. There are two bootstrap
paths:

**Instant bootstrap (forward price available):**

1. `DataEngine` receives `SubscribeOptionChain`, resolves all instruments for the
   series from the cache, and requests forward prices from the data client.
2. When the forward price response arrives, the engine creates the manager with
   the ATM price pre-seeded. The manager computes the active strike set during
   construction.
3. The engine subscribes the active instruments immediately.

**Deferred bootstrap (no forward price):**

1. Same as above, but no matching forward price is found in the response.
2. The engine creates the manager with no initial ATM price. The active set is
   empty and no wire subscriptions are made for the chain.
3. Bootstrap depends on relevant Greeks data already flowing from other
   subscriptions (e.g., per-instrument `subscribe_option_greeks` calls). When
   the engine feeds an `OptionGreeks` event with `underlying_price` through
   `handle_greeks()`, the manager bootstraps and returns `True`. The engine
   then subscribes the now-active instrument set.

Once bootstrapped, the aggregator monitors ATM drift. On each snapshot timer tick,
the engine calls `check_rebalance()` which returns any instruments to add or
remove. A hysteresis threshold and cooldown period prevent thrashing near strike
boundaries.

## OptionGreeks data type

`OptionGreeks` carries venue-provided sensitivities and implied volatility for a
single option contract:

| Field              | Type             | Description                                           |
|--------------------|------------------|-------------------------------------------------------|
| `instrument_id`    | `InstrumentId`   | The option contract these Greeks apply to.             |
| `delta`            | `float`          | Rate of change of option price per unit underlying.    |
| `gamma`            | `float`          | Rate of change of delta per unit underlying.           |
| `vega`             | `float`          | Sensitivity to a 1% change in implied volatility.      |
| `theta`            | `float`          | Daily time decay (dV/dt / 365.25).                     |
| `rho`              | `float`          | Sensitivity to a change in interest rate.              |
| `mark_iv`          | `float` or None  | Mark implied volatility.                               |
| `bid_iv`           | `float` or None  | Bid implied volatility.                                |
| `ask_iv`           | `float` or None  | Ask implied volatility.                                |
| `underlying_price` | `float` or None  | Underlying price at time of calculation.               |
| `open_interest`    | `float` or None  | Open interest for the contract.                        |
| `ts_event`         | `int`            | UNIX timestamp (nanoseconds) of the event.             |
| `ts_init`          | `int`            | UNIX timestamp (nanoseconds) when initialized.         |

## OptionChainSlice data type

`OptionChainSlice` is a point-in-time snapshot of an entire option series.

Properties:

| Property     | Type                 | Description                              |
|--------------|----------------------|------------------------------------------|
| `series_id`  | `OptionSeriesId`     | The option series identifier.            |
| `atm_strike` | `Price` or None      | Current ATM strike (if determined).      |
| `ts_event`   | `int`                | UNIX timestamp (nanoseconds).            |
| `ts_init`    | `int`                | UNIX timestamp (nanoseconds).            |

Call and put data are accessed through methods, not as direct properties.
Each `OptionStrikeData` returned by these methods contains a `quote` (`QuoteTick`)
and an optional `greeks` (`OptionGreeks`) for that strike.

Methods:

- `strikes()`: all unique strike prices in the chain.
- `strike_count()`, `call_count()`, `put_count()`: counts.
- `get_call(strike)`, `get_put(strike)`: full `OptionStrikeData`.
- `get_call_greeks(strike)`, `get_put_greeks(strike)`: Greeks only.
- `get_call_quote(strike)`, `get_put_quote(strike)`: quote only.
- `is_empty()`: true if the chain has no data.

## Adapter support

The following adapters currently support option Greeks subscriptions:

| Adapter | Per‑instrument Greeks | Option chains |
|---------|:---------------------:|:-------------:|
| Deribit | ✓                     | ✓             |
| Bybit   | ✓                     | ✓             |
| OKX     | ✓                     | -             |

## See also

- [Greeks](greeks.md) - Local Greeks calculation and portfolio risk management.
- [Data](data.md) - Built-in data types and the subscription model.
- [Actors](actors.md) - Subscription and handler reference table.
