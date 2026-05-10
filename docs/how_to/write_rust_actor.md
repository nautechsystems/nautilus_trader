# Write an Actor (Rust)

An actor receives market data, custom data/signals, and system events but does not manage orders.
This guide walks through building a `SpreadMonitor` that subscribes to quotes
and logs the bid-ask spread.

For background on actors, traits, and handler dispatch, see the
[Actors](../concepts/actors.md) and [Rust](../concepts/rust.md) concept guides.

## Define the struct

An actor owns a `DataActorCore` and any state it needs. The core provides
subscription methods, cache access, and clock access through `Deref`.

```rust
use nautilus_common::{nautilus_actor, actor::{DataActor, DataActorConfig, DataActorCore}};
use nautilus_model::{data::QuoteTick, identifiers::{ActorId, InstrumentId}};

pub struct SpreadMonitor {
    core: DataActorCore,
    instrument_id: InstrumentId,
}
```

## Implement the constructor

Create a `DataActorConfig` with an actor ID, then pass it to `DataActorCore::new`.
The config fields use `Option` with defaults, so `..Default::default()` covers
everything except the actor ID.

```rust
impl SpreadMonitor {
    pub fn new(instrument_id: InstrumentId) -> Self {
        let config = DataActorConfig {
            actor_id: Some(ActorId::from("SPREAD_MON-001")),
            ..Default::default()
        };
        Self {
            core: DataActorCore::new(config),
            instrument_id,
        }
    }
}
```

## Wire up the core and implement Debug

The `nautilus_actor!` macro generates the `Deref<Target = DataActorCore>`
and `DerefMut` impls that give your struct direct access to subscription
methods, cache, and clock. By default it delegates to a field named `core`;
pass a second argument for a different field name.

`Debug` is a trait bound on `DataActor` (required by the blanket `Component`
impl), so implement it manually or derive it.

```rust
nautilus_actor!(SpreadMonitor);

impl std::fmt::Debug for SpreadMonitor {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SpreadMonitor").finish()
    }
}
```

## Implement the DataActor trait

Override handler methods to receive data. All handlers have default no-op
implementations, so you only override what you need. Each handler returns
`anyhow::Result<()>`.

```rust
impl DataActor for SpreadMonitor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        self.subscribe_quotes(self.instrument_id, None, None);
        Ok(())
    }

    fn on_quote(&mut self, quote: &QuoteTick) -> anyhow::Result<()> {
        let spread = quote.ask_price.as_f64() - quote.bid_price.as_f64();
        log::info!("Spread: {spread:.5}");
        Ok(())
    }
}
```

`subscribe_quotes` is available directly on `self` because of the `Deref` to
`DataActorCore`. See the
[handler table](../concepts/rust.md#handler-methods) for all available
handlers.

## Register the actor

With a `BacktestEngine`:

```rust
let actor = SpreadMonitor::new(instrument_id);
engine.add_actor(actor)?;
```

With a `LiveNode`:

```rust
let actor = SpreadMonitor::new(instrument_id);
node.add_actor(actor)?;
```

## Guard safety

When the system dispatches messages to your actor, it obtains a short-lived
`ActorRef` guard from the registry. You do not manage these guards directly.
If you write code that accesses other actors in a callback, follow these
rules:

- Look up actors by ID each time; do not cache an `ActorRef`.
- Drop the guard before the scope ends; never store it in a field.
- Never hold a guard across an `.await` point.

The subscription methods on `DataActorCore` handle this correctly by
capturing the actor ID and performing the lookup inside the callback closure.
See [Runtime invariants](../developer_guide/rust.md#runtime-invariants) for
the full threading and registry model.

## Full example

See
[`BookImbalanceActor`](https://github.com/nautechsystems/nautilus_trader/tree/develop/crates/trading/src/examples/actors/imbalance)
for a more complete actor that tracks per-instrument state and prints a
summary on stop.
