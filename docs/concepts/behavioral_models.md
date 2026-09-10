# Behavioral Models

This page describes the pluggable models that change or extend NautilusTrader behavior.

A behavioral model supplies the rules for a specific calculation or decision made by the system.
For example, a fill model determines simulated fill eligibility and liquidity, while a fee model
calculates the commission on a fill. The engine calls the configured model through a defined
interface, so users can change these rules without modifying the engine or their strategy.

Models are supplied as objects in configuration or through runtime APIs. Users can select a
built-in implementation, configure its parameters, or supply a custom implementation where the
model family and API support it. Here, pluggable means replacing behavior through these interfaces;
[runtime loading of native libraries](#native-extension-boundary) is a separate capability.

## Model families

| Family  | Controls                                                                             | Built-in examples                                             |
| ------- | ------------------------------------------------------------------------------------ | ------------------------------------------------------------- |
| Fill    | Simulated limit-fill eligibility, slippage, and optional synthetic liquidity.        | `DefaultFillModel`, `BestPriceFillModel`, `TwoTierFillModel`. |
| Fee     | Commission calculated from an order, fill, instrument, and optional pricing context. | `MakerTakerFeeModel`, `FixedFeeModel`, `PerContractFeeModel`. |
| Latency | Simulated delays for order submission, modification, and cancellation.               | `StaticLatencyModel`.                                         |
| Margin  | Initial order margin and maintenance position margin.                                | `StandardMarginModel`, `LeveragedMarginModel`.                |

Fill models range from using the recorded book to supplying synthetic liquidity with tiered,
partial-fill, size-aware, competition-aware, volume-sensitive, or market-hours behavior. See
[fill models](backtesting/fill-models.md) for the complete built-in list and the effect of book type
on fill simulation.

Fee models also include `ProbabilityPriceFeeModel`, `CappedOptionFeeModel`, and
`TieredNotionalOptionFeeModel` for probability-priced instruments and option fee schedules.
`StaticLatencyModel` adds a base delay to separately configured insert, update, and cancel delays.
The [margin models](backtesting/accounts-and-margin.md#margin-models) select whether instrument
margin requirements are reduced by account leverage.

Fill and latency models control simulated execution; they do not determine whether or when a live
venue fills an order. Model behavior applies at the engine or account boundary that consumes it.
Changing a model does not replace order-state validation, execution routing, or the rest of the
runtime.

## Simulation modules

[Simulation modules](backtesting/simulation-modules.md) provide a related extension point for
behavior across the simulated exchange lifecycle. Instead of answering a fill, fee, latency, or
margin calculation, a module processes exchange state and returns account adjustments. Built-in
FX rollover and CFD swap modules use this interface. Custom modules can extend simulation behavior
through the same lifecycle and acknowledgement contract.

## Supplying implementations

Rust callers can implement the model family's trait and pass the implementation through its runtime
handle. Python support depends on both the family and the configuration API:

| Family  | `BacktestVenueConfig` from Python                    | `BacktestEngine.add_venue()` from Python             |
| ------- | ---------------------------------------------------- | ---------------------------------------------------- |
| Fill    | Built-in model objects.                              | Built-in models or custom Python fill-model objects. |
| Fee     | Built-in models or custom Python commission objects. | Built-in models or custom Python commission objects. |
| Latency | `StaticLatencyModel`.                                | `StaticLatencyModel`.                                |
| Margin  | `StandardMarginModel` or `LeveragedMarginModel`.     | `StandardMarginModel` or `LeveragedMarginModel`.     |

A custom Python fill model implements the [fill-model protocol](backtesting/fill-models.md#configuration).
A custom Python fee model supplies `get_commission`; it can also provide
`get_commission_with_context` when its calculation needs the underlying price.
The engine invokes these methods when it needs the corresponding decision or calculation.

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
dependency. Low-level Rust code passes these models through the corresponding handle. Python
exposure uses an explicit bridge for the model family, either as an enum variant or as a trait
implementation passed through the handle, depending on the storage boundary. Latency and margin
configuration accept built-in models only from Python.

## Dispatch boundary

| Form                     | Accepted implementations               | Dispatch     | Role                                          |
| ------------------------ | -------------------------------------- | ------------ | --------------------------------------------- |
| Concrete type or generic | One concrete implementation            | Static       | Model internals and specialized callers.      |
| `<Family>ModelAny`       | Declared built-ins and bridge variants | Enum match   | Built-in and bridge configuration or storage. |
| `<Family>ModelHandle`    | Any accepted Rust trait implementation | Trait object | Shared runtime storage and custom types.      |

`<Family>ModelAny` uses enum dispatch. `<Family>ModelHandle` uses dynamic dispatch through a trait
object, so a built-in converted from the enum into a handle crosses a vtable before its enum match.
Built-in-only storage remains typed as `<Family>ModelAny` where avoiding trait-object dispatch
matters. The handle has no separate built-in fast path; the simpler single representation remains
because no measured performance case justifies the additional variant and dispatch complexity.

## Native extension boundary

The open-source [plug-in crate](../developer_guide/plugins.md) defines an artifact ABI, but model registration and the
loading host are not part of this repository. The open-source distribution does not provide runtime
native model plugins. Native models are composed at compile time and passed through the
corresponding enum or handle.

[Simulation modules](backtesting/simulation-modules.md) use these representations at
backtest configuration and runtime boundaries.
