# Strategy Lifecycle

A strategy is a `Component` with a well-defined state machine. This page
traces the lifecycle from registration through runtime data flow to shutdown.

## State machine

Every strategy passes through these states. Triggers move the state forward;
invalid triggers for the current state return an error.

```
PreInitialized ──Initialize──▶ Ready
                                 │
                               Start
                                 │
                                 ▼
                             Starting
                           ╱         ╲
                StartCompleted      Fault
                     │                │
                     ▼                ▼
                 Running           Faulting ──▶ Faulted
               ╱    │    ╲
          Degrade  Stop   Fault
            │       │
            ▼       ▼
        Degrading  Stopping
            │     ╱       ╲
            ▼  StopCompleted  Fault
        Degraded    │
         │  │       ▼
    Resume Stop   Stopped
      │    │     ╱   │   ╲
      ▼    │  Reset Resume Dispose
   Resuming│    │     ▲      │
      │    │    ▼     │      ▼
      │    │  Resetting   Disposing
      │    │    │            │
      │    │  Ready      Disposed
      ▼    ▼
    Running
```

| From             | Trigger          | To               |
|------------------|------------------|------------------|
| PreInitialized   | Initialize       | Ready            |
| Ready            | Start            | Starting         |
| Starting         | StartCompleted   | Running          |
| Starting         | Fault            | Faulting         |
| Running          | Stop             | Stopping         |
| Running          | Degrade          | Degrading        |
| Running          | Fault            | Faulting         |
| Stopping         | StopCompleted    | Stopped          |
| Stopped          | Resume           | Resuming         |
| Stopped          | Reset            | Resetting        |
| Stopped          | Dispose          | Disposing        |
| Resuming         | ResumeCompleted  | Running          |
| Degrading        | DegradeCompleted | Degraded         |
| Degraded         | Resume           | Resuming         |
| Degraded         | Stop             | Stopping         |
| Resetting        | ResetCompleted   | Ready            |
| Disposing        | DisposeCompleted | Disposed         |
| Faulting         | FaultCompleted   | Faulted          |

## Registration

Call chain: **user code → `LiveNode::add_strategy()` → `Trader::add_strategy()`**

```
user code:
    node.add_strategy(my_strategy)?;

LiveNode::add_strategy()               [crates/live/src/node/mod.rs]
├── check state == Idle (reject if running)
├── kernel.trader.prepare_strategy_for_registration(&mut strategy)
│   └── assigns trader_id, wires clock + cache into StrategyCore
├── register_component_actor(strategy)  [crates/common/src/component.rs]
│   ├── Rc<UnsafeCell<T>> wraps the strategy
│   ├── component registry[component_id] = Rc clone
│   └── actor registry[actor_id] = Rc clone
├── register_external_order_claims() (if configured)
└── kernel.exec_engine.register_oms_type() (if configured)
```

State: `PreInitialized → Ready` (transition happens inside `register()`).

:::warning
Call `add_strategy()` **before** `node.start()`. The node rejects strategies
added while running.
:::

## Startup

Call chain: **`LiveNode::start()` → `Kernel::start_trader()` → `Trader::start_with_component_callbacks()` → `start_component()` → `Component::start()` → `on_start()`**

```
LiveNode::start()                       [crates/live/src/node/mod.rs]
├── set NodeState::Starting
├── kernel.start_async()
├── kernel.connect_data_clients()       ← fetches instruments
├── runner.flush_pending_data()         ← instruments into cache
├── kernel.connect_exec_clients()       ← needs instruments in cache
├── await_engines_connected()
├── perform_startup_reconciliation()    ← sync open orders/positions
└── kernel.start_trader()               [crates/system/src/kernel.rs]
    ├── order_emulator.start()
    └── Trader::start_with_component_callbacks()  [crates/system/src/trader.rs]
        ├── trader.transition(Start)    ← Trader itself: Ready → Starting
        ├── for actor_id in actor_ids:
        │   └── start_component(&actor_id)
        ├── for strategy_id in strategy_ids:
        │   └── start_component(&strategy_id)  [crates/common/src/component.rs]
        │       ├── lookup component registry
        │       └── component.start()
        │           ├── transition(Start)           ← Ready → Starting
        │           ├── on_start()                  ← YOUR CODE RUNS HERE
        │           │   └── subscribe_trades(), subscribe_quotes(), etc.
        │           └── transition(StartCompleted)  ← Starting → Running
        ├── for exec_algorithm_id in exec_algorithm_ids:
        │   └── start_component(&exec_algorithm_id)
        └── trader.transition(StartCompleted)  ← Trader: Starting → Running
```

If `on_start()` returns an error, the strategy stays in `Starting` and does NOT
transition to `Running`. The error propagates up and the node logs the failure.

## Runtime

Once running, the data and execution engines route events to the strategy
through the actor registry.

| Event                      | Callback                 | Source            |
|----------------------------|--------------------------|-------------------|
| Market trade               | `on_trade()`             | Data engine.      |
| Quote update               | `on_quote()`             | Data engine.      |
| Bar close                  | `on_bar()`               | Data engine.      |
| Order fill/reject/cancel   | `on_order_*()` events    | Execution engine. |
| Position open/close        | `on_position_*()` events | Execution engine. |
| Timer fires                | `handle_time_event()`    | Clock.            |
| Custom data                | `on_data()`              | Data engine.      |

Submit orders via `self.submit_order()`, which routes through the risk engine
before reaching the execution engine and adapter.

## Shutdown

Call chain: **`LiveNode` shutdown → `Kernel::stop_trader()` → `Trader::stop_components()` → `stop_component()` → `Component::stop()` → `on_stop()`**

```
LiveNode (shutdown sequence)            [crates/live/src/node/mod.rs]
└── kernel.stop_trader()                [crates/system/src/kernel.rs]
    └── Trader::stop_components()       [crates/system/src/trader.rs]
        ├── for strategy_id in strategy_ids:
        │   └── stop_component(&strategy_id)  [crates/common/src/component.rs]
        │       ├── lookup component registry
        │       └── component.stop()
        │           ├── transition(Stop)            ← Running → Stopping
        │           ├── on_stop()                   ← YOUR CODE RUNS HERE
        │           │   └── unsubscribe_trades(), cancel orders, etc.
        │           └── transition(StopCompleted)   ← Stopping → Stopped
        ├── for actor_id in actor_ids:
        │   └── stop_component(&actor_id)
        └── for exec_algorithm_id in exec_algorithm_ids:
            └── stop_component(&exec_algorithm_id)
```

## Resume

A stopped strategy can resume:

1. Transition: `Stopped → Resuming`.
2. Call `on_resume()`.
3. Transition: `Resuming → Running`.

## Degraded mode

The system transitions a strategy to degraded when a non-fatal issue occurs
(for example, a data feed drops):

1. Transition: `Running → Degrading → Degraded`.
2. From degraded the strategy can be resumed or stopped.

## Fault

Any callback error can trigger a fault:

1. Transition: `<current> → Faulting → Faulted`.
2. Faulted is a terminal state — the strategy cannot recover.

## Trait hierarchy

Implement `DataActor` and `Debug` on your strategy struct. The
`nautilus_strategy!` macro generates `DataActorNative` and `StrategyNative`.
Blanket implementations then provide `Actor`, `Component`, and `Strategy`
automatically.

```
YourStrategy
  ├── DataActor          (you implement: on_start, on_trade, on_stop)
  ├── DataActorNative    (macro-generated)
  ├── Debug              (you implement)
  │
  └── blanket impls provide:
      ├── Actor           (id, handle, as_any)
      ├── Component       (register, state, transition_state)
      └── Strategy        (submit_order, order, portfolio)
```

## Example

```rust
use nautilus_common::actor::DataActor;
use nautilus_model::data::TradeTick;
use nautilus_model::identifiers::InstrumentId;
use nautilus_trading::{nautilus_strategy, strategy::StrategyCore};

pub struct MyStrategy {
    core: StrategyCore,
    instrument_id: InstrumentId,
}

nautilus_strategy!(MyStrategy);

impl std::fmt::Debug for MyStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MyStrategy")
            .field("instrument_id", &self.instrument_id)
            .finish()
    }
}

impl DataActor for MyStrategy {
    fn on_start(&mut self) -> anyhow::Result<()> {
        let id = self.instrument_id;
        self.subscribe_trades(id, None, None);
        Ok(())
    }

    fn on_trade(&mut self, tick: &TradeTick) -> anyhow::Result<()> {
        // React to market data
        Ok(())
    }

    fn on_stop(&mut self) -> anyhow::Result<()> {
        let id = self.instrument_id;
        self.unsubscribe_trades(id, None, None);
        Ok(())
    }
}
```

## Key locations

| Concept                             | File                                    |
|-------------------------------------|-----------------------------------------|
| State machine                       | `crates/common/src/component.rs`        |
| `Actor` / `Component` blanket impls | `crates/common/src/actor/data_actor.rs` |
| `DataActor` trait (callbacks)       | `crates/common/src/actor/data_actor.rs` |
| `Strategy` trait (order API)        | `crates/trading/src/strategy/mod.rs`    |
| `nautilus_strategy!` macro          | `crates/trading/src/macros.rs`          |
| `Trader::add_strategy()`           | `crates/system/src/trader.rs`           |
| `LiveNode::add_strategy()`         | `crates/live/src/node/mod.rs`           |
| Global registries                   | `crates/common/src/component.rs`        |
