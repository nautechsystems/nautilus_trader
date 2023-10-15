# Backtesting

Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation. This system is composed of various components
including [Actors](advanced/actors.md), [Strategies](strategies.md), [Execution Algorithms](execution.md),
and other user-defined modules. The entire trading simulation is predicated on a stream of historical data processed by a
`BacktestEngine`. Once this data stream is exhausted, the engine concludes its operation, producing 
detailed results and performance metrics for in-depth analysis.

It's paramount to recognize that NautilusTrader offers two distinct API levels for setting up and 
conducting backtests: **high-level** and **low-level**.

## Choosing an API level:

Consider the **low-level** API when:

- The entirety of your data stream can be comfortably accommodated within available memory
- You choose to avoid storing data in the Nautilus-specific Parquet format
- Or, you have a specific need/preference for retaining raw data in its innate format, such as CSV, Binary, etc
- You seek granular control over the `BacktestEngine`, enabling functionalities such as re-running backtests on identical data while interchanging components (like actors or strategies) or tweaking parameter settings

Consider the **high-level** API when:

- Your data stream's size exceeds available memory, necessitating streaming data in batches
- You want to harness the performance capabilities and convenience of the `ParquetDataCatalog` and persist your data in the Nautilus-specific Parquet format
- You value the flexibility and advanced functionalities offered by passing configuration objects, which can define diverse backtest runs across many engines at once

## Low-level API:

The low-level API revolves around a single `BacktestEngine`, with inputs initialized and added 'manually' via a Python script.
An instantiated `BacktestEngine` can accept:
- Lists of `Data` objects which will be automatically sorted into monotonic order by `ts_init`
- Multiple venues (manually initialized and added)
- Multiple actors (manually initialized and added)
- Multiple execution algorithms (manually initialized and added)

## High-level API:

The high-level API revolves around a single `BacktestNode`, which will orchestrate the management 
of individual `BacktestEngine`s, each defined by a `BacktestRunConfig`.
Multiple configurations can be bundled into a list and fed to the node to be run.

Each of these `BacktestRunConfig` objects in turn is made up of:
- A list of `BacktestDataConfig` objects
- A list of `BacktestVenueConfig` objects
- A list of `ImportableActorConfig` objects
- A list of `ImportableStrategyConfig` objects
- A list of `ImportableExecAlgorithmConfig` objects
- An optional `ImportableControllerConfig` object
- An optional `BacktestEngineConfig` object (otherwise will be the default)

**This doc is an evolving work in progress and will continue to describe each API more fully...**
