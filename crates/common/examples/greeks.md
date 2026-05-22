# Greeks Calculator Integration with Actor System

This document explains how to use the `GreeksCalculator` with the Nautilus actor system.

## Overview

The `GreeksCalculator` is a utility for calculating option and futures greeks (sensitivities of price moves with respect to market data moves). It has been integrated with the actor system to allow for easy use within actors, including strategies.

## Key Components

1. **Clock**: The `GreeksCalculator` uses the same `Clock` instance as the actor system.
2. **Message Bus**: The `GreeksCalculator` uses the messaging switchboard for publishing and subscribing to messages.

## Using GreeksCalculator in an Actor

### Basic Setup

```rust
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use nautilus_common::{
    actor::{
        data_actor::{DataActor, DataActorConfig, DataActorCore},
        Actor,
    },
    cache::Cache,
    greeks::{GreeksCalculator, InstrumentGreeksParams},
    live::clock::LiveClock,
    msgbus::MessagingSwitchboard,
};

struct MyActor {
    core: DataActorCore,
    greeks_calculator: GreeksCalculator,
}

impl MyActor {
    pub fn new(
        config: DataActorConfig,
        cache: Rc<RefCell<Cache>>,
        clock: Rc<RefCell<LiveClock>>,
        switchboard: Arc<MessagingSwitchboard>,
    ) -> Self {
        let core = DataActorCore::new(config, cache.clone(), clock.clone(), switchboard.clone());

        // Create the GreeksCalculator with the same clock and cache
        let greeks_calculator = GreeksCalculator::new(
            cache,
            clock,
        );

        Self {
            core,
            greeks_calculator,
        }
    }
}
```

### Calculating Greeks

```rust
use nautilus_model::{
    data::{CustomData, greeks::GreeksData},
    identifiers::InstrumentId,
};

impl MyActor {
    pub fn calculate_greeks(&self, instrument_id: InstrumentId) -> anyhow::Result<GreeksData> {
        InstrumentGreeksParams::builder()
            .instrument_id(instrument_id)
            .cache_greeks(true)
            .publish_greeks(true)
            .ts_event(self.core.clock.borrow().timestamp_ns())
            .build()
            .calculate(&self.greeks_calculator)
    }
}
```

### Subscribing to Greeks Data

```rust
impl MyActor {
    pub fn subscribe_to_greeks(&self, underlying: &str) {
        self.greeks_calculator
            .subscribe_greeks(underlying, Some(Self::handle_greeks as fn(&GreeksData)));
    }

    fn handle_greeks(greeks: &GreeksData) {
        println!("Received greeks data: {greeks:?}");
    }
}

impl DataActor for MyActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        // Subscribe to greeks data for SPY
        self.subscribe_to_greeks("SPY");
        Ok(())
    }

    fn on_data(&mut self, data: &CustomData) -> anyhow::Result<()> {
        println!("Received custom data: {}", data.data_type);
        Ok(())
    }
}
```

## Full Example

See the complete example in `crates/common/examples/greeks_actor_example.rs` for a working implementation.

## Key Features

1. **Integration with Actor System**: The `GreeksCalculator` uses the same clock and message bus as the actor system.
2. **Message Bus Integration**: Greeks data can be published and subscribed to via the message bus.
3. **Caching**: Greeks calculations can be cached for performance.
4. **Portfolio Greeks**: Calculate greeks for an entire portfolio of positions.

## Notes

- When setting `publish_greeks` to `true`, the calculator publishes typed `GreeksData` to the message bus with a topic format of `data.GreeksData.instrument_id={symbol}`.
- Greeks subscriptions are handled through `subscribe_greeks`; `DataActor::on_data` receives `CustomData` wrappers and is not the greeks delivery path.
- When subscribing to greeks data, you can provide a custom handler or use the default handler which caches the received greeks data.
