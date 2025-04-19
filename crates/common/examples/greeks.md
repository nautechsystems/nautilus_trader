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
    clock::LiveClock,
    greeks::GreeksCalculator,
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
    data::greeks::GreeksData,
    identifiers::InstrumentId,
};

impl MyActor {
    pub fn calculate_greeks(&self, instrument_id: InstrumentId) -> anyhow::Result<GreeksData> {
        // Example parameters
        let flat_interest_rate = 0.0425;
        let flat_dividend_yield = None;
        let spot_shock = 0.0;
        let vol_shock = 0.0;
        let time_to_expiry_shock = 0.0;
        let use_cached_greeks = false;
        let cache_greeks = true;
        let publish_greeks = true;
        let ts_event = self.core.clock.borrow().timestamp_ns();
        let position = None;
        let percent_greeks = false;
        let index_instrument_id = None;
        let beta_weights = None;

        // Calculate greeks
        self.greeks_calculator.instrument_greeks(
            instrument_id,
            Some(flat_interest_rate),
            flat_dividend_yield,
            Some(spot_shock),
            Some(vol_shock),
            Some(time_to_expiry_shock),
            Some(use_cached_greeks),
            Some(cache_greeks),
            Some(publish_greeks),
            Some(ts_event),
            position,
            Some(percent_greeks),
            index_instrument_id,
            beta_weights,
        )
    }
}
```

### Subscribing to Greeks Data

```rust
impl MyActor {
    pub fn subscribe_to_greeks(&self, underlying: &str) {
        // Subscribe to greeks data
        self.greeks_calculator.subscribe_greeks(underlying, None);
    }
}

impl DataActor for MyActor {
    fn on_start(&mut self) -> anyhow::Result<()> {
        // Subscribe to greeks data for SPY
        self.subscribe_to_greeks("SPY");
        Ok(())
    }

    fn on_data(&mut self, data: &dyn std::any::Any) -> anyhow::Result<()> {
        // Handle received data
        if let Some(greeks_data) = data.downcast_ref::<GreeksData>() {
            println!("Received greeks data: {:?}", greeks_data);
        }
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

- When setting `publish_greeks` to `true`, the calculator will publish the greeks data to the message bus with a topic format of `data.GreeksData.instrument_id={symbol}`.
- When subscribing to greeks data, you can provide a custom handler or use the default handler which caches the received greeks data.
