# API Reference

Welcome to the API reference for the Python/Cython implementation of NautilusTrader!

The API reference is automatically generated from the latest NautilusTrader source 
code from the repositories `develop` branch, using [sphinx](https://www.sphinx-doc.org/en/master/).

```{note}
Given the platforms development is still within an extended **beta** phase, at
a future time we may separate the documentation between the `develop` branch, and
more stable releases on `master`.
```

## Type Safety
The design of the platform holds software correctness and safety at the highest level.
Given most of the core production code is written in Cython, type safety is often provided
at the C level.

```{note}
If a function or methods parameter is not explicitly typed as allowing
``None``, then you can assume you will receive a `ValueError` when passing ``None``
as an argument (this is not explicitly documented).
```

## Framework Organization
The codebase is organized around both layering of abstraction levels, and generally
grouped into logical subpackages of cohesive concepts. You can navigate to the documentation
for each of these subpackages from the left menu.

### Core / Low-Level
- `core`: constants, functions and low-level components used throughout the framework
- `common`: common parts for assembling the frameworks various components
- `network`: low-level base components for networking clients
- `serialization`: serialization base components and serializer implementations
- `model`: defines a rich trading domain model

### System Components
- `accounting`: different account types and account management machinery
- `adapters`: integration adapters for the platform including brokers and exchanges
- `analysis`: components relating to trading performance statistics and analysis
- `cache`: provides common caching infrastructure
- `data`: the data stack and data tooling for the platform
- `execution`: the execution stack for the platform
- `indicators`: a set of efficient indicators and analyzers
- `infrastructure`: technology specific infrastructure implementations
- `msgbus`: a universal message bus for connecting system components
- `persistence`: data storage, cataloging and retrieval, mainly to support backtesting
- `portfolio`: portfolio management functionality
- `risk`: risk specific components and tooling
- `trading`: trading domain specific components and tooling

### System Implementations
- `backtest`: backtesting componentry as well as a backtest engine implementation
- `live`: live engine and client implementations as well as a node for live trading
- `system`: the core system kernel common between backtest, sandbox and live contexts

## Errors and Exceptions
Every attempt has been made to accurately document the possible exceptions which
can be raised from NautilusTrader code, and the conditions which will trigger them.

```{warning}
There may be other undocumented exceptions which can be raised by Pythons standard 
library, or from third party library dependencies.
```


```{eval-rst}
.. toctree::
   :maxdepth: 1
   :glob:
   :titlesonly:
   :hidden:
   
   accounting.md
   adapters/index.md
   analysis.md
   backtest.md
   cache.md
   common.md
   config.md
   core.md
   data.md
   execution.md
   indicators.md
   infrastructure.md
   live.md
   model/index.md
   msgbus.md
   network.md
   persistence.md
   portfolio.md
   risk.md
   serialization.md
   system.md
   trading.md
```
