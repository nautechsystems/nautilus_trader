# Concepts

```{eval-rst}
.. toctree::
   :maxdepth: 1
   :glob:
   :titlesonly:
   :hidden:
   
   overview.md
   architecture.md
   strategies.md
   instruments.md
   orders.md
   execution.md
   backtesting.md
   data.md
   adapters.md
   logging.md
   advanced/index.md
```

Welcome to NautilusTrader!


Explore the foundational concepts of NautilusTrader through the following guides.

```{note}
The terms "NautilusTrader", "Nautilus" and "platform" are used interchageably throughout the documentation.
```

## [Overview](overview.md)
The **Overview** guide covers the main use cases for the platform.

## [Architecture](architecture.md)
The **Architecture** guide dives deep into the foundational principles, structures, and designs that underpin
the platform. Whether you're a developer, system architect, or just curious about the inner workings 
of NautilusTrader.

## [Strategies](strategies.md)
The heart of the NautilusTrader user experience is in writing and working with
trading strategies. The **Strategies** guide covers how to implement trading strategies for the platform.

## [Instruments](instruments.md)
The `Instrument` base class represents the core specification for any tradable asset/contract.

## [Orders](orders.md)
The **Orders** guide provides more details about the available order types for the platform, along with
the execution instructions supported for each.

## [Execution](execution.md)
NautilusTrader can handle trade execution and order management for multiple strategies and venues
simultaneously (per instance). Several interacting components are involved in execution, making it 
crucial to understand the possible flows of execution messages (commands and events).

## [Backtesting](backtesting.md)
Backtesting with NautilusTrader is a methodical simulation process that replicates trading
activities using a specific system implementation.

## [Data](data.md)
The NautilusTrader platform defines a range of built-in data types crafted specifically to represent 
a trading domain

## [Adapters](adapters.md)
The NautilusTrader design allows for integrating data publishers and/or trading venues
through adapter implementations, these can be found in the top level `adapters` subpackage. 

## [Logging](logging.md)
The platform provides logging for both backtesting and live trading using a high-performance logger implemented in Rust.

## [Advanced](advanced/index.md)
Here you will find more detailed documentation and examples covering the more advanced
features and functionality of the platform.

```{note}
It's important to note that the [API Reference](../api_reference/index.md) documentation should be 
considered the source of truth for the platform. If there are any discrepancies between concepts described here
and the API Reference, then the API Reference should be considered the correct information. We are 
working to ensure that concepts stay up-to-date with the API Reference and will be introducing 
doc tests in the near future to help with this.
```
