# Options Data and Greeks (Bybit)

:::note
This is a **Rust-only** v2 system tutorial. It uses the Rust `LiveNode` with
the Bybit adapter to stream live option Greeks and option chain snapshots.
:::

This tutorial connects to Bybit's live options market and streams Greeks and
aggregated option chain snapshots using a Rust `DataActor`. It covers
instrument discovery, venue-provided Greeks subscriptions, and periodic
option chain snapshots with ATM-relative strike filtering.

## Introduction

Bybit publishes Greeks (delta, gamma, vega, theta) and implied volatility
alongside every option ticker update. NautilusTrader exposes this data at two
levels:

- **Per-instrument Greeks**: subscribe to a single option contract and receive
  an `OptionGreeks` event on every ticker update.
- **Option chain snapshots**: subscribe to an entire expiry series and receive
  periodic `OptionChainSlice` events that aggregate quotes and Greeks across
  all active strikes.

Two example binaries in the Bybit adapter crate back these patterns: the first
subscribes to individual Greeks streams; the second subscribes to an aggregated
option chain with ATM-relative strike filtering.

## Prerequisites

- A working Rust toolchain ([rustup.rs](https://rustup.rs)).
- The NautilusTrader repository cloned and building.
- A Bybit API key with read permissions. No trading permissions are required
  for data-only use. Create keys at
  [bybit.com](https://www.bybit.com/app/user/api-management).
- Environment variables set for authentication:

```bash
export BYBIT_API_KEY="your-api-key"
export BYBIT_API_SECRET="your-api-secret"
```

A `.env` file in the repository root also works. The examples load it via
`dotenvy`.

:::warning
Bybit's demo environment does not support options. The public WebSocket
endpoint for options does not exist on `stream-demo.bybit.com`. Use mainnet
or testnet credentials for this tutorial.
:::

## The DataActor pattern

A Rust `DataActor` needs three pieces:

1. A struct with a `core: DataActorCore` field plus your own state.
2. The `nautilus_actor!(YourType)` macro plus a `Debug` implementation.
3. A `DataActor` trait implementation with your callbacks.

The macro generates blanket `Actor` and `Component` implementations, so you
only implement the callbacks you need. Every callback has a default no-op
implementation.

## Part 1: per-instrument Greeks

The `bybit-greeks-tester` example subscribes to `OptionGreeks` for all BTC
CALL options at the nearest expiry and logs each update.

### Actor structure

```rust
#[derive(Debug)]
struct GreeksTester {
    core: DataActorCore,
    client_id: ClientId,
    subscribed_instruments: Vec<InstrumentId>,
}

nautilus_actor!(GreeksTester);

impl GreeksTester {
    fn new(client_id: ClientId) -> Self {
        Self {
            core: DataActorCore::new(DataActorConfig {
                actor_id: Some("GREEKS_TESTER-001".into()),
                ..Default::default()
            }),
            client_id,
            subscribed_instruments: Vec::new(),
        }
    }
}
```

The `core` field is required by the macro. The `client_id` identifies which
data client to route subscriptions to. The `subscribed_instruments` vector
tracks what we subscribed to so we clean up on stop.

### Discovering instruments

On start, the actor queries the cache for all option instruments, filters for
BTC CALLs that have not expired, and finds the nearest expiry:

```rust
fn on_start(&mut self) -> anyhow::Result<()> {
    let venue = Venue::new("BYBIT");
    let underlying_filter = Ustr::from("BTC");

    let mut options: Vec<(InstrumentId, f64, u64)> = {
        let cache = self.cache();
        let instruments = cache.instruments(&venue, Some(&underlying_filter));

        instruments
            .iter()
            .filter_map(|inst| {
                if inst.option_kind() == Some(OptionKind::Call) {
                    let expiry = inst.expiration_ns()?.as_u64();
                    let strike = inst.strike_price()?.as_f64();
                    Some((inst.id(), strike, expiry))
                } else {
                    None
                }
            })
            .collect()
    }; // cache borrow dropped here

    let now_ns = self.timestamp_ns().as_u64();
    options.retain(|(_, _, exp)| *exp > now_ns);

    let nearest_expiry = options.iter().map(|(_, _, exp)| *exp).min().unwrap();
    options.retain(|(_, _, exp)| *exp == nearest_expiry);
    options.sort_by(|(_, a, _), (_, b, _)| a.partial_cmp(b).unwrap());

    // ...subscribe to each
}
```

:::warning
Release the cache borrow before calling any subscription methods. The cache
uses `Rc<RefCell<...>>` internally, and subscription methods may need to
borrow it. Collect owned data into a local `Vec`, drop the cache reference,
then subscribe.
:::

### Subscribing to Greeks

After discovering instruments, subscribe to each one:

```rust
let client_id = self.client_id;
for (instrument_id, _, _) in &options {
    self.subscribe_option_greeks(*instrument_id, Some(client_id), None);
    self.subscribed_instruments.push(*instrument_id);
}
```

### Handling updates

Each ticker update from Bybit triggers `on_option_greeks` with an
`OptionGreeks` event:

```rust
fn on_option_greeks(&mut self, greeks: &OptionGreeks) -> anyhow::Result<()> {
    log::info!(
        "GREEKS | {} | delta={:.4} gamma={:.6} vega={:.4} theta={:.4} rho={:.6} | \
         mark_iv={} bid_iv={} ask_iv={} | underlying={} oi={}",
        greeks.instrument_id,
        greeks.delta,
        greeks.gamma,
        greeks.vega,
        greeks.theta,
        greeks.rho,
        greeks.mark_iv.map_or("-".to_string(), |v| format!("{v:.2}")),
        greeks.bid_iv.map_or("-".to_string(), |v| format!("{v:.2}")),
        greeks.ask_iv.map_or("-".to_string(), |v| format!("{v:.2}")),
        greeks.underlying_price.map_or("-".to_string(), |v| format!("{v:.2}")),
        greeks.open_interest.map_or("-".to_string(), |v| format!("{v:.1}")),
    );
    Ok(())
}
```

The `OptionGreeks` fields:

| Field              | Type           | Description                                        |
|--------------------|----------------|----------------------------------------------------|
| `instrument_id`    | `InstrumentId` | The option contract.                               |
| `delta`            | `f64`          | Price sensitivity to underlying.                   |
| `gamma`            | `f64`          | Delta sensitivity to underlying.                   |
| `vega`             | `f64`          | Price sensitivity to a 1% change in volatility.    |
| `theta`            | `f64`          | Daily time decay.                                  |
| `rho`              | `f64`          | Sensitivity to interest rate changes.              |
| `mark_iv`          | `Option<f64>`  | Mark price implied volatility.                     |
| `bid_iv`           | `Option<f64>`  | Bid implied volatility.                            |
| `ask_iv`           | `Option<f64>`  | Ask implied volatility.                            |
| `underlying_price` | `Option<f64>`  | Current underlying forward price for this expiry.  |
| `open_interest`    | `Option<f64>`  | Open interest for this contract.                   |

The `delta`, `gamma`, `vega`, `theta`, and `rho` values live on a nested
`greeks: OptionGreekValues` struct. `OptionGreeks` implements `Deref<Target = OptionGreekValues>`,
so `greeks.delta` and friends work as shown above.

Bybit does not provide rho; the adapter sets it to `0.0`.

### Cleanup

On stop, unsubscribe from all instruments:

```rust
fn on_stop(&mut self) -> anyhow::Result<()> {
    let ids: Vec<InstrumentId> = self.subscribed_instruments.drain(..).collect();
    let client_id = self.client_id;
    for instrument_id in ids {
        self.unsubscribe_option_greeks(instrument_id, Some(client_id), None);
    }
    log::info!("Unsubscribed from all option greeks");
    Ok(())
}
```

## Part 2: option chain snapshots

The `bybit-option-chain` example subscribes to an aggregated option chain and
logs periodic snapshots showing calls and puts at each strike with their
quotes and Greeks.

### Why use option chains?

Per-instrument subscriptions give granular control, but monitoring an entire
surface means managing individual streams and correlating updates across
strikes. An option chain subscription handles this: the `DataEngine`
aggregates quotes and Greeks across all strikes in a series and publishes a
single `OptionChainSlice` on a timer.

This aggregation happens inside NautilusTrader. Bybit publishes per-contract
option market data and does not expose a native option chain stream in the V5
public WebSocket docs.

### Key types

**`OptionSeriesId`** identifies a single expiry series:

```rust
let series_id = OptionSeriesId::new(
    Venue::new("BYBIT"),    // venue
    Ustr::from("BTC"),      // underlying
    Ustr::from("USDT"),     // settlement currency
    UnixNanos::from(expiry), // expiration timestamp
);
```

**`StrikeRange`** controls which strikes are active:

| Variant       | Description                                            |
|---------------|--------------------------------------------------------|
| `Fixed`       | A fixed set of strike prices.                          |
| `AtmRelative` | `strikes_above` above and `strikes_below` below ATM.   |
| `AtmPercent`  | All strikes within `pct` of the ATM price.             |

For ATM-based variants, subscriptions are deferred until the ATM price is
determined from the venue-provided forward price.

### Subscribing

```rust
let strike_range = StrikeRange::AtmRelative {
    strikes_above: 3,
    strikes_below: 3,
};

let snapshot_interval_ms = Some(5_000); // snapshot every 5 seconds

self.subscribe_option_chain(
    series_id,
    strike_range,
    snapshot_interval_ms,
    Some(client_id),
    None, // params
);
```

Pass `None` for `snapshot_interval_ms` to use raw mode, where every quote or
Greeks update publishes a slice immediately.

### Handling snapshots

The `on_option_chain` callback receives an `OptionChainSlice` containing all
active strikes with their call and put data:

```rust
fn on_option_chain(&mut self, slice: &OptionChainSlice) -> anyhow::Result<()> {
    log::info!(
        "OPTION_CHAIN | {} | atm={} | calls={} puts={} | strikes={}",
        slice.series_id,
        slice.atm_strike.map_or("-".to_string(), |p| format!("{p}")),
        slice.call_count(),
        slice.put_count(),
        slice.strike_count(),
    );

    for strike in slice.strikes() {
        let call_info = slice.get_call(&strike).map(|d| {
            let greeks_str = d.greeks.as_ref().map_or("-".to_string(), |g| {
                format!(
                    "d={:.3} g={:.5} v={:.2} iv={:.1}%",
                    g.delta, g.gamma, g.vega,
                    g.mark_iv.unwrap_or(0.0) * 100.0,
                )
            });
            format!("bid={} ask={} [{}]", d.quote.bid_price, d.quote.ask_price, greeks_str)
        });

        let put_info = slice.get_put(&strike).map(|d| {
            let greeks_str = d.greeks.as_ref().map_or("-".to_string(), |g| {
                format!(
                    "d={:.3} g={:.5} v={:.2} iv={:.1}%",
                    g.delta, g.gamma, g.vega,
                    g.mark_iv.unwrap_or(0.0) * 100.0,
                )
            });
            format!("bid={} ask={} [{}]", d.quote.bid_price, d.quote.ask_price, greeks_str)
        });

        log::info!(
            "  K={} | CALL: {} | PUT: {}",
            strike,
            call_info.unwrap_or_else(|| "-".to_string()),
            put_info.unwrap_or_else(|| "-".to_string()),
        );
    }

    Ok(())
}
```

The `OptionChainSlice` fields and methods:

| Name             | Type / Returns              | Description                          |
|------------------|-----------------------------|--------------------------------------|
| `series_id`      | `OptionSeriesId`            | The series this snapshot covers.     |
| `atm_strike`     | `Option<Price>`             | ATM strike from the forward price.   |
| `call_count()`   | `usize`                     | Number of call strikes with data.    |
| `put_count()`    | `usize`                     | Number of put strikes with data.     |
| `strike_count()` | `usize`                     | Union of all strikes.                |
| `strikes()`      | `Vec<Price>`                | Sorted list of all strike prices.    |
| `get_call(k)`    | `Option<&OptionStrikeData>` | Call quote and Greeks at strike `k`. |
| `get_put(k)`     | `Option<&OptionStrikeData>` | Put quote and Greeks at strike `k`.  |

Each `OptionStrikeData` contains a `quote: QuoteTick` (bid/ask) and an
optional `greeks: Option<OptionGreeks>`.

## Node setup

Both examples use the same `LiveNode` pattern. No execution client is needed
for data-only use:

```rust
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    dotenvy::dotenv().ok();

    let environment = Environment::Live;
    let trader_id = TraderId::test_default();
    let client_id = ClientId::new("BYBIT");

    let bybit_config = BybitDataClientConfig {
        api_key: None,    // loaded from BYBIT_API_KEY env var
        api_secret: None, // loaded from BYBIT_API_SECRET env var
        product_types: vec![BybitProductType::Option],
        ..Default::default()
    };

    let client_factory = BybitDataClientFactory::new();

    let mut node = LiveNode::builder(trader_id, environment)?
        .with_name("BYBIT-OPTIONS-001".to_string())
        .add_data_client(None, Box::new(client_factory), Box::new(bybit_config))?
        .with_delay_post_stop_secs(5)
        .build()?;

    let actor = GreeksTester::new(client_id); // or OptionChainTester
    node.add_actor(actor)?;
    node.run().await?;

    Ok(())
}
```

Setting `product_types` to `[BybitProductType::Option]` loads only option
instruments. Startup blocks while the instrument provider fetches and parses
every listed option.

## Results

### Greeks output

The Greeks tester logs one line per ticker update:

```
Found 22 BTC CALL options at nearest expiry (ts=1775289600000000000)
Subscribed to option greeks for 22 instruments
GREEKS | BTC-4APR26-66500-C-USDT-OPTION.BYBIT | delta=0.7619 gamma=0.000160 vega=5.8873 theta=-88.2596 rho=0.000000 | mark_iv=0.30 bid_iv=0.00 ask_iv=0.30 | underlying=66902.63 oi=13.4
GREEKS | BTC-4APR26-67000-C-USDT-OPTION.BYBIT | delta=0.4282 gamma=0.000220 vega=7.4646 theta=-103.2034 rho=0.000000 | mark_iv=0.28 bid_iv=0.13 ask_iv=0.15 | underlying=66902.63 oi=90.0
GREEKS | BTC-4APR26-67500-C-USDT-OPTION.BYBIT | delta=0.1290 gamma=0.000118 vega=4.0030 theta=-55.1456 rho=0.000000 | mark_iv=0.28 bid_iv=0.18 ask_iv=0.22 | underlying=66902.63 oi=222.8
```

### Option chain output

The chain tester logs a summary line and one row per strike on each snapshot
interval:

```
Found 44 BTC options at nearest expiry (ts=1775289600000000000, settlement=USDT)
Subscribing to option chain: BYBIT-BTC-USDT-1775289600000000000
OPTION_CHAIN | BYBIT-BTC-USDT-1775289600000000000 | atm=67000.0 | calls=6 puts=6 | strikes=7
  K=64000.0 | CALL: bid=2780.0 ask=2830.0 [d=0.920 g=0.00006 v=2.66 iv=33.0%] | PUT: bid=5.0 ask=12.0 [d=-0.080 g=0.00006 v=2.66 iv=34.2%]
  K=65000.0 | CALL: bid=1870.0 ask=1920.0 [d=0.849 g=0.00012 v=4.82 iv=31.5%] | PUT: bid=15.0 ask=30.0 [d=-0.151 g=0.00012 v=4.82 iv=32.8%]
  K=66000.0 | CALL: bid=1050.0 ask=1090.0 [d=0.689 g=0.00018 v=6.89 iv=30.1%] | PUT: bid=70.0 ask=95.0 [d=-0.311 g=0.00018 v=6.89 iv=31.0%]
  K=67000.0 | CALL: bid=410.0 ask=440.0 [d=0.428 g=0.00022 v=7.46 iv=28.4%] | PUT: bid=350.0 ask=380.0 [d=-0.572 g=0.00022 v=7.46 iv=29.2%]
  K=68000.0 | CALL: bid=90.0 ask=115.0 [d=0.160 g=0.00015 v=5.20 iv=29.0%] | PUT: bid=1020.0 ask=1060.0 [d=-0.840 g=0.00015 v=5.20 iv=29.8%]
  K=69000.0 | CALL: bid=10.0 ask=25.0 [d=0.038 g=0.00005 v=1.83 iv=31.2%] | PUT: bid=1940.0 ask=1980.0 [d=-0.962 g=0.00005 v=1.83 iv=32.0%]
```

## Running the examples

```bash
# Per-instrument Greeks
cargo run --example bybit-greeks-tester --package nautilus-bybit

# Option chain snapshots
cargo run --example bybit-option-chain --package nautilus-bybit
```

Stop either example with Ctrl+C. The actor's `on_stop` callback unsubscribes
from all streams before shutdown.

## Complete source

- [`crates/adapters/bybit/examples/node_greeks.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/bybit/examples/node_greeks.rs)
- [`crates/adapters/bybit/examples/node_option_chain.rs`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/adapters/bybit/examples/node_option_chain.rs)

## Next steps

- **Combine both patterns**: Use per-instrument Greeks for near-ATM contracts
  alongside the aggregated chain view in a single actor. Subscribe to Greeks
  for contracts you want to track individually, and the chain for a
  surface-level view.
- **Add quote and depth subscriptions**: Call `subscribe_quotes` for
  top-of-book `QuoteTick` updates on individual option contracts. Call
  `subscribe_order_book_deltas` when you need the dedicated option orderbook
  stream. Bybit supports option depths 25 and 100.
- **Options execution**: The
  [delta-neutral strategy tutorial](delta_neutral_options_bybit.md) walks
  through a short strangle with perpetual hedging, including IV-based order
  placement via Bybit's `order_iv` parameter.

## See also

- [Options](../concepts/options.md) - Option instrument types, Greeks data
  types, and chain architecture.
- [Bybit integration](../integrations/bybit.md) - Full Bybit adapter
  reference including options order parameters and limitations.
