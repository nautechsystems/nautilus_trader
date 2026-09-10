# Simulation Modules

This page describes simulation module configuration, lifecycle, and failure handling.

The [behavioral model design](../behavioral_models.md) explains the enum and
shared-handle representations used below.

Simulation modules use the enum and handle forms at different configuration boundaries:

| Boundary                                       | Stored form              | Accepted implementations              |
| ---------------------------------------------- | ------------------------ | ------------------------------------- |
| Declarative `BacktestVenueConfig`              | `SimulationModuleAny`    | Built-ins and language bridges.       |
| `SimulatedVenueConfig` and `SimulatedExchange` | `SimulationModuleHandle` | Any linked Rust trait implementation. |

`SimulationModuleHandle` owns an `Rc<dyn SimulationModule>`, so cloning a handle shares the module
and its state. Cloning a built-in enum value copies its state, while cloning a Python bridge retains
the same Python object. Venues or runs that require isolated state therefore use distinct module
instances, including distinct Python objects.

## Lifecycle

The exchange runs each module through this lifecycle:

1. `pre_process` runs before the exchange processes each supported market data item.
1. `process` runs for each module in order against the same read-only exchange snapshot after
   commands have settled for the timestamp. Processing stops at the first failure, and the exchange
   applies no adjustments from that timestamp.
1. For each completed result in order, the exchange applies its batch as ordered `Money`
   adjustments, then calls that module's `acknowledge` exactly once with the corresponding outcomes,
   including for an empty batch.

## Failure handling

- Failures from `pre_process`, `process`, `acknowledge`, or `reset` leave the exchange in an error
  state until every module resets successfully. This prevents a failed acknowledgement from
  replaying adjustments that the account may already contain.
- Diagnostic failures return to the engine with the module index and hook name without changing the
  exchange error state.

## Python modules

The `process` hook for a Python `SimulationModule` subclass receives an owned
`SimulationModuleContext` snapshot containing:

- The venue.
- The optional base currency.
- The instruments.
- The order books.
- The open positions.

The bridge does not expose mutable cache or matching-engine state. Python exceptions retain the hook
name as they propagate through the exchange and `BacktestEngine.run`.

## Linked native types

Linked native PyO3 types can register an extractor for their Python class. The extractor resolves an
object for imperative `BacktestEngine.add_venue` configuration. Python configuration resolves
modules as follows:

| Configuration path         | Accepted objects                                      | Stored form              | Native extractor behavior  |
| -------------------------- | ----------------------------------------------------- | ------------------------ | -------------------------- |
| `BacktestEngine.add_venue` | Built-ins, linked native types, and Python subclasses | `SimulationModuleHandle` | Matches the exact type.    |
| `BacktestVenueConfig`      | Built-ins and Python subclasses                       | `SimulationModuleAny`    | Does not consult registry. |

An unrelated class with the same name does not select a registered extractor. Extractor
registration does not create a runtime ABI for trait objects across a `cdylib` boundary.

## Built-in modules

The built-in FX rollover and CFD swap modules use the completed-batch acknowledgement flow. CFD swap
rates are per-instrument signed daily `Decimal` fractions of settlement notional, with separate long
and short values, a configurable UTC rollover time, and a configurable triple-roll weekday. For a
single-currency account, the module converts the adjustment to the account base currency at the
cached mid exchange rate.

The CFD swap module defers the whole batch when any of these inputs is missing:

- A matching engine.
- A settlement price.
- An exchange rate.

The module logs one warning per booking date, instrument, and failure kind before quieter retries.

Perpetual funding remains part of `SimulatedExchange` and is not a simulation module.
