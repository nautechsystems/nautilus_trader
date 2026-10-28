# Behavioral Models

This page describes the pluggable models that change or extend NautilusTrader behavior.

A **behavioral model** supplies the rules for a specific calculation or decision made by the system.
For example, a fill model determines simulated fill eligibility and liquidity, while a fee model
calculates the commission on a fill. The engine calls the configured model through a defined
interface, so users can change these rules without modifying the engine or their strategy.

Models are supplied as objects in configuration or through runtime APIs. Users can select a built-in
implementation, configure its parameters, or supply a custom implementation where the model family and API
support it. Here, pluggable means that users replace behavior through these interfaces.
[runtime loading of native libraries](#native-extension-boundary) is a separate capability.

## Model families

| Family  | Controls                                                                             | Built-in examples                                             |
| ------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| Fill    | Simulated limit-fill eligibility, slippage, and optional synthetic liquidity.        | `DefaultFillModel`, `BestPriceFillModel`, `TwoTierFillModel`. |
| Fee     | Commission calculated from an order, fill, instrument, and optional pricing context. | `MakerTakerFeeModel`, `FixedFeeModel`, `PerContractFeeModel`. |
| Margin  | Initial order margin and maintenance position margin.                                | `StandardMarginModel`, `LeveragedMarginModel`.                |
| Latency | Simulated delays for order submission, modification, and cancellation.               | `StaticLatencyModel`.                                         |

Fill models range from using the recorded book to supplying synthetic liquidity with tiered,
partial-fill, size-aware, competition-aware, volume-sensitive, or market-hours behavior. See
[fill models](backtesting/fill-models.md) for the complete built-in list and the effect of book type
on fill simulation.

Fee models also include `ProbabilityPriceFeeModel`, `CappedOptionFeeModel`, and `TieredNotionalOptionFeeModel`
for probability-priced instruments and option fee schedules.

The [margin models](backtesting/accounts-and-margin.md#margin-models) select whether instrument margin
requirements are reduced by account leverage. `StaticLatencyModel` adds a base delay to separately configured
insert, update, and cancel delays.

Fill and latency models control simulated execution. They do not determine whether or when a live venue fills
an order. Model behavior applies at the engine or account boundary that consumes it. Changing a model does not
replace order-state validation, execution routing, or the rest of the runtime.

## Simulation modules

[Simulation modules](backtesting/simulation-modules.md) provide a related extension point for
behavior across the simulated exchange lifecycle. Instead of answering a fill, fee, margin, or
latency calculation, a module processes exchange state and returns account adjustments. Built-in
FX rollover and CFD swap modules use this interface. Custom modules can extend simulation behavior
through the same lifecycle and acknowledgement contract.

## Supplying implementations

Rust callers can implement the model family's trait and pass the implementation through its runtime
handle. Python callers can supply built-in or custom models through these APIs. Model parameters follow
the order `fill_model`, `fee_model`, `margin_model`, and `latency_model`. When available, `modules` follows
the model group. Use keyword arguments to keep calls independent of parameter order.

| Family  | `BacktestVenueConfig`                    | `BacktestEngine.add_venue()`             | `SandboxExecutionClientConfig`           |
| ------- | ---------------------------------------- | ---------------------------------------- | ---------------------------------------- |
| Fill    | Built-in models or custom Python models. | Built-in models or custom Python models. | Built-in models or custom Python models. |
| Fee     | Built-in models or custom Python models. | Built-in models or custom Python models. | Built-in models or custom Python models. |
| Margin  | Built-in models or custom Python models. | Built-in models or custom Python models. | Built-in models or custom Python models. |
| Latency | Built-in models or custom Python models. | Built-in models or custom Python models. | Built-in models or custom Python models. |

Configuration copies share the supplied custom Python model instance. Its random generator, replay state,
and other state persist across runs and `engine.reset()`. For an independent run, reset that state yourself
or construct a new engine with a fresh model.

### Custom Python fill models

Pass a `FillModel` subclass or a compatible Python object as `fill_model` to
`BacktestVenueConfig`, `BacktestEngine.add_venue()`, `BacktestEngine.change_fill_model(venue, fill_model)`,
or `SandboxExecutionClientConfig`. See [custom fill models](backtesting/fill-models.md#custom-fill-models)
for the required methods, optional spread-fill behavior, and liquidity hook.

### Custom Python fee models

Implement `get_commission(order, fill_quantity, fill_px, instrument)` and return a `Money` commission
for each fill. Subclass `FeeModel` or supply a Python object with this method, then pass the instance as
`fee_model` to `BacktestVenueConfig`, `BacktestEngine.add_venue()`, or `SandboxExecutionClientConfig`.

When the calculation needs the underlying price, implement `get_commission_with_context`.
It receives the same arguments and an additional `underlying_px`. The engine supplies an underlying price
only for `CryptoOption` and `OptionContract` instruments when one is available; otherwise, it passes `None`.
Without an override, the engine calls `get_commission`.

### Custom Python margin models

Pass a subclass of `nautilus_trader.model.MarginModel` or a compatible Python object as `margin_model`
to these APIs. See [Python model selection](accounting.md#python-model-selection) for the callback contract.

### Custom Python latency models

Subclass `LatencyModel` and override all three methods. Each method returns an `int` delay in
`[0, 2**64 - 1]` nanoseconds, including any base latency; the engine adds none. Boolean values are invalid.

```python
from random import Random

from nautilus_trader.execution import LatencyModel


class SampledLatency(LatencyModel):
    def __init__(self, seed=42):
        self._rng = Random(seed)

    def get_insert_latency(self, command, ts_now):
        return self._rng.choice((1_000_000, 2_000_000, 5_000_000))

    def get_update_latency(self, command, ts_now):
        return self._rng.choice((500_000, 1_000_000))

    def get_delete_latency(self, command, ts_now):
        return self._rng.choice((250_000, 500_000))
```

Pass an instance as `latency_model` to `BacktestVenueConfig`, `BacktestEngine.add_venue()`, or
`SandboxExecutionClientConfig`. Custom objects that do not subclass `LatencyModel` raise `TypeError`.
An unimplemented callback raises `NotImplementedError` when called and triggers the failure handling below.

In backtests, the engine calls the model only for queued commands. Venues created from `BacktestVenueConfig`
always queue commands. `BacktestEngine.add_venue()` queues commands by default; with `use_message_queue=False`,
it processes them immediately without calling the latency model.

Each callback receives the concrete command and `ts_now`, the current clock time in Unix nanoseconds:
simulation time in backtests and the sandbox client's clock time in sandbox.

| Method               | Command types                                          |
| -------------------- | ------------------------------------------------------ |
| `get_insert_latency` | `SubmitOrder`, `SubmitOrderList`.                      |
| `get_update_latency` | `ModifyOrder`, `BatchModifyOrders`.                    |
| `get_delete_latency` | `CancelOrder`, `BatchCancelOrders`, `CancelAllOrders`. |

Import command classes from `nautilus_trader.live`. For single-order commands, use
`command.client_order_id` to look up measured delays. The same methods receive whole batch commands and
return one delay for all their members. The engine calls the model once per command.

Backtest arrival time is `command.ts_init + delay`; the engine processes the command at the first eligible
[settlement point](backtesting/execution-flow.md#latency-delayed-commands) at or after that time.
Sandbox arrival time is `ts_now + delay`, where `ts_now` is the receipt time.

Callback errors, invalid delays, and arrival-time overflow have these effects:

- **`BacktestEngine`**: `run()` or `end()` raises `RuntimeError` and aborts the run. Call `engine.reset()`
  before the next run.
- **`BacktestNode`**: the node logs the error and omits that run's result by default. With `raise_exception=True`
  in `BacktestRunConfig`, it raises the error instead.
- **Sandbox**: the client rejects failed submits and emits modify or cancel rejections for targets that remain
  open. It logs and discards a failed `CancelAllOrders`.

## Model family structure

Behavioral model families use a common representation across simulation and execution:

- A Rust `<Family>Model` trait defines the behavioral contract.
- Concrete Rust types implement the built-in models.
- A `<Family>ModelAny` enum lists the core built-ins and any language bridges that require enum
  storage, then implements the trait through explicit enum dispatch.
- A `<Family>ModelHandle` stores a shared trait object where runtime components accept linked Rust
  implementations beyond the enum variants.

Supported concrete built-ins are exposed as PyO3 classes. Their
[type stub annotations](../developer_guide/rust.md#type-stub-annotations) feed the
[generated Python artifacts](../developer_guide/rust.md#generated-python-artifacts). Backtest configuration accepts
these concrete model objects directly rather than using separate model configuration and factory
wrappers.

Adapter-specific models live in their adapter crate when a core enum variant would create a reverse
dependency. Low-level Rust code passes these models through the corresponding handle.

Python exposure uses an explicit bridge for the model family. Depending on the storage boundary, this is an
enum variant or a trait implementation passed through the handle.

## Dispatch boundary

| Form                     | Accepted implementations               | Dispatch     | Role                                          |
| ------------------------ | -------------------------------------- | ------------ | --------------------------------------------- |
| Concrete type or generic | One concrete implementation            | Static       | Model internals and specialized callers.      |
| `<Family>ModelAny`       | Declared built-ins and bridge variants | Enum match   | Built-in and bridge configuration or storage. |
| `<Family>ModelHandle`    | Any accepted Rust trait implementation | Trait object | Shared runtime storage and custom types.      |

`<Family>ModelAny` uses enum dispatch. `<Family>ModelHandle` uses dynamic dispatch through a trait object, so
a built-in converted from the enum into a handle crosses a vtable before its enum match. Built-in-only storage
stays typed as `<Family>ModelAny` where avoiding trait-object dispatch matters.

The handle has no separate built-in fast path. The handle uses the simpler single representation because no
measured performance case justifies the additional variant and dispatch complexity.

## Native extension boundary

The open-source [plug-in crate](../developer_guide/plugins.md) defines an artifact ABI, but model registration and the
loading host are not part of this repository. The open-source distribution does not provide runtime
native model plugins. Native models are composed at compile time and passed through the
corresponding enum or handle.

[Simulation modules](backtesting/simulation-modules.md) use these representations at
backtest configuration and runtime boundaries.
