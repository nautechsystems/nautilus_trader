# Book Imbalance Backtest with Betfair Data (Rust)

:::note
This is a **Rust-only** v2 system tutorial. No Python, no Cython, no Parquet catalog.
It uses the Rust `BacktestEngine` directly with raw Betfair streaming data.
:::

This tutorial backtests a **book imbalance** actor on Betfair exchange data.
It loads raw historical streaming data from a `.gz` file, feeds it through
the `BacktestEngine`, and runs a `DataActor` that tracks bid/ask volume
imbalance per runner.

## Introduction

Betfair is a sports betting exchange where participants back (bid) and lay
(ask) outcomes at decimal odds. The exchange order book for each runner
(selection) behaves like a financial order book. This makes it a natural
fit for NautilusTrader.

Book imbalance measures whether more quoted volume appears on the bid or ask
side of the book. For each batch of order book deltas, we sum the resting
size at each updated price level per side and compute:

```
imbalance = (bid_volume - ask_volume) / (bid_volume + ask_volume)
```

A positive value means more backing interest for the outcome. Sports
traders use this as a building-block signal, combining it with price
momentum or market-wide features.

This example uses the Rust backtest engine directly, without Python or the
Parquet catalog. A release build processes ~3 million data points per
second with full order book maintenance in the matching engine.

## Prerequisites

- A working Rust toolchain (see [rustup.rs](https://rustup.rs)).
- The NautilusTrader repository cloned and building.
- A Betfair historical `.gz` file containing MCM (Market Change Message) data.
  Obtain from the Betfair historic data site, a third-party provider, or
  record your own via the Exchange Streaming API.

Place the data file at:

```
tests/test_data/local/betfair/1.253378068.gz
```

This path is gitignored and not shipped with the repository. The example
dataset used below is a football MATCH_ODDS market with 3 runners and
~82,000 MCM lines recorded over 18 days.

## Loading the data

`BetfairDataLoader` reads gzip-compressed Betfair Exchange Streaming API files
and parses each line into Nautilus domain objects:

```rust
use nautilus_betfair::loader::{BetfairDataItem, BetfairDataLoader};
use nautilus_model::types::Currency;

let mut loader = BetfairDataLoader::new(Currency::GBP(), None);
let items = loader.load(&filepath)?;
```

The loader returns a `Vec<BetfairDataItem>` with these variants:

| Variant             | Description                                     | Maps to `Data` enum?       |
|:--------------------|:------------------------------------------------|:---------------------------|
| `Instrument`        | Runner definition from market definition.       | No (added separately)      |
| `Status`            | Market status transition (PreOpen, Trading...). | No (`Data` has no variant) |
| `Deltas`            | Order book snapshot or delta update.            | ✓ `Data::Deltas`           |
| `Trade`             | Incremental trade tick from cumulative volumes. | ✓ `Data::Trade`            |
| `Ticker`            | Last traded price, volume, BSP near/far.        | -                          |
| `StartingPrice`     | Betfair Starting Price for a runner.            | -                          |
| `BspBookDelta`      | BSP‑specific book delta.                        | -                          |
| `InstrumentClose`   | Settlement event.                               | ✓ `Data::InstrumentClose`  |
| `SequenceCompleted` | Batch completion marker.                        | -                          |
| `RaceRunnerData`    | GPS tracking data (horse/greyhound racing).     | -                          |
| `RaceProgress`      | Race‑level progress data.                       | -                          |

The backtest engine accepts the `Data` enum, so we convert the items we need
and skip the Betfair-specific types:

```rust
use nautilus_model::data::{Data, OrderBookDeltas_API};

let mut instruments = AHashMap::new();
let mut data: Vec<Data> = Vec::new();

for item in items {
    match item {
        BetfairDataItem::Instrument(inst) => {
            instruments.insert(inst.id(), *inst);
        }
        BetfairDataItem::Deltas(d) => {
            data.push(Data::Deltas(OrderBookDeltas_API::new(d)));
        }
        BetfairDataItem::Trade(t) => {
            data.push(Data::Trade(t));
        }
        BetfairDataItem::InstrumentClose(c) => {
            data.push(Data::InstrumentClose(c));
        }
        _ => {} // Betfair-specific types, not handled here
    }
}
```

`OrderBookDeltas_API` is a thin wrapper around `OrderBookDeltas` required by
the `Data` enum (an FFI shim).

Instruments are re-emitted on every market definition update in the stream,
so the map naturally deduplicates them by keeping the latest version.

:::warning
The `Status` variant carries market status transitions (PreOpen, Trading,
Suspended, Closed) but the `Data` enum has no variant for it. This example
does not replay status transitions. If you extend this into a strategy that
places orders, the matching engine will not see market suspensions or
closures from the stream. Handle this by subscribing to instrument status
separately or adding status routing to the engine.
:::

## The actor

NautilusTrader ships with a `BookImbalanceActor` in the trading crate's
examples module. The example imports it directly:

```rust
use nautilus_trading::examples::actors::BookImbalanceActor;

let actor = BookImbalanceActor::new(instrument_ids, 5000, None);
engine.add_actor(actor)?;
```

The second argument is the log interval: print a progress line every 5000
updates. Set to 0 to disable periodic logging.

The full source is at
[`crates/trading/src/examples/actors/imbalance.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/trading/src/examples/actors/imbalance.rs).

### How it works

A `DataActor` in Rust needs three pieces:

1. A struct holding a `DataActorCore` field plus your own state.
2. `nautilus_actor!(YourType)` to wire up the core, plus a `Debug` implementation.
3. The `DataActor` trait implementation with your callbacks.

The framework provides blanket `Actor` and `Component` implementations for
any type that implements `DataActor + Debug`, so you do not need to implement
those manually.

On start, the actor subscribes to `OrderBookDeltas` for each instrument. On
each update, it sums the volume per side from the individual deltas and
accumulates running totals. On stop, it prints a per-instrument summary.

Setting `managed: false` in `subscribe_book_deltas` means the data engine does
not maintain a separate order book copy in the cache for the actor. The
exchange-side matching engine still maintains its own book (via
`book.apply_delta()` on every delta). Set `managed: true` if your actor needs
to read the full book state from `self.cache().order_book(&instrument_id)`.

## Backtest engine setup

### Create the engine and venue

Betfair is a cash-settled betting exchange. We configure the venue with
`AccountType::Cash`, `OmsType::Netting`, and `BookType::L2_MBP` for the
L2 order book data:

```rust
let mut engine = BacktestEngine::new(BacktestEngineConfig::default())?;

engine.add_venue(
    Venue::from("BETFAIR"),
    OmsType::Netting,
    AccountType::Cash,
    BookType::L2_MBP,
    vec![Money::from("1_000_000 GBP")],
    None,            // base_currency
    None,            // default_leverage
    AHashMap::new(), // per-instrument leverages
    None,            // margin_model
    vec![],          // simulation modules
    FillModelAny::default(),
    FeeModelAny::default(),
    // ... remaining options default to None
)?;
```

### Add instruments, actor, and data

```rust
for instrument in instruments.values() {
    engine.add_instrument(instrument)?;
}

let actor = BookImbalanceActor::new(instrument_ids, 5000, None);
engine.add_actor(actor)?;

engine.add_data(data, None, true, true);
```

The `add_data` parameters are `(data, client_id, validate, sort)`. With
`validate: true` the engine checks that instruments are registered for each
data point. With `sort: true` it sorts by timestamp.

### Run

```rust
engine.run(None, None, None, false)?;
```

The four parameters are `(start, end, run_config_id, streaming)`. Passing
`None` for start/end uses the full time range of the loaded data.

## What happens during the run

For each data point in timestamp order, the engine:

1. Advances the clock to the data timestamp.
2. Routes the data to the simulated exchange, which applies each delta to the
   per-instrument `OrderBook` and runs the matching engine cycle.
3. Publishes the data through the data engine and message bus, triggering
   the actor's `on_book_deltas` callback.
4. Drains command queues and settles venues (processes any pending orders).

The matching engine maintains a full order book for each instrument. This
example has no orders to match. The book state is ready for order matching
when the actor is replaced with a `Strategy`.

## Results

With the example football MATCH_ODDS dataset (3 runners, ~143k data points),
the release build completes in ~48ms:

```
--- Book imbalance summary ---
  1.253378068-2426.BETFAIR   updates: 53197  bid_vol: 212225339.34  ask_vol: 117422531.85  imbalance: 0.2876
  1.253378068-48783.BETFAIR   updates: 36475  bid_vol: 52506905.49   ask_vol: 19104694.72   imbalance: 0.4664
  1.253378068-58805.BETFAIR   updates: 25426  bid_vol: 24295351.82   ask_vol: 25692733.11   imbalance: -0.0280
```

Runner 2426 (the eventual winner, settled at BSP 2.22) shows a persistent
positive backing imbalance of +0.29 throughout the market lifetime.

## Running the example

```bash
# Debug build
cargo run -p nautilus-betfair --example betfair-backtest

# Release build (recommended)
cargo run -p nautilus-betfair --release --example betfair-backtest

# Custom data file
cargo run -p nautilus-betfair --release --example betfair-backtest -- path/to/file.gz
```

## Complete source

The complete example is available at
[`crates/adapters/betfair/examples/betfair_backtest.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/betfair/examples/betfair_backtest.rs).

## Next steps

- **Add a Strategy**: Replace the actor with a `Strategy` implementation that
  places back/lay orders based on the imbalance signal. See the `EmaCross`
  example in `crates/trading/src/examples/strategies/ema_cross.rs` for the
  pattern.
- **Use managed books**: Set `managed: true` in `subscribe_book_deltas` and
  access the full book via `self.cache().order_book(&id)` for richer signals
  like top-of-book spread, depth ratios, or weighted mid-price.
- **Multiple markets**: Load several `.gz` files and run them through the
  same engine to test cross-market signals.
- **Compare with Python**: Run the same backtest from Python using the
  `BacktestEngine` Python API. The Rust engine processes the same data
  pipeline at roughly 6x the throughput of the Python/Cython path.
