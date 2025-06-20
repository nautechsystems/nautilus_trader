# Engineering Overview: nautilus_trader Codebase

This document provides an architectural and functional overview of the `nautilus_trader` codebase for future agents and contributors. It summarizes the main subsystems, their responsibilities, and tips for navigating the project.

---

## High-Level Structure

The `nautilus_trader` package is organized into the following main submodules:

- **accounting/**: Account management, calculations, and error handling for financial operations.
- **adapters/**: Integrations with external trading venues, data providers, and APIs. Contains venue-specific adapters (e.g., Binance, Bybit, Polymarket, Betfair, etc.).
- **analysis/**: Tools for analyzing trading data, including statistical analysis and reporting.
- **backtest/**: Simulation framework for running historical trading strategies and evaluating performance.
- **cache/**: Infrastructure for caching data and improving performance.
- **common/**: Shared utilities, constants, and helpers used across the codebase.
- **config/**: Configuration management for the system.
- **core/**: Core trading engine components, including order matching, engine logic, and data models.
- **data/**: Data ingestion, transformation, and management.
- **examples/**: Example scripts and notebooks for using the framework.
- **execution/**: Order execution logic and integration with trading venues.
- **indicators/**: Technical indicators and signal processing tools.
- **live/**: Live trading infrastructure and runtime management.
- **model/**: Financial models, instruments, and portfolio abstractions.
- **persistence/**: Database and storage logic.
- **portfolio/**: Portfolio management and risk tracking.
- **risk/**: Risk management modules.
- **serialization/**: Data serialization and deserialization.
- **system/**: System-level utilities and orchestrators.
- **test_kit/**: Test utilities and fixtures for robust testing.
- **trading/**: High-level trading strategy and orchestration logic.

---

## Key Subsystems

### Adapters

- Each adapter (e.g., `polymarket`, `binance`, `betfair`) implements the interface for connecting to and interacting with a specific trading venue or data provider.
- Venue adapters typically provide methods for market data subscription, order placement, and account management.

### Core

- Contains the trading engine, matching logic, and core data models (orders, trades, accounts, etc.).
- Implements the event-driven architecture that powers backtesting and live trading.

### Backtest

- Enables historical simulation of trading strategies using recorded market data.
- Useful for performance evaluation and strategy research.

### Execution

- Handles order routing, execution management, and integration with adapters.

### Model/Portfolio/Risk

- Provides abstractions for financial instruments, positions, portfolios, and risk controls.

### Indicators/Analysis

- Implements a wide range of technical indicators and statistical analysis tools for strategy development.

### Live

- Manages live trading sessions, real-time data feeds, and runtime orchestration.

---

## Design Patterns & Best Practices

- **Modularity**: Each subsystem is designed to be as independent as possible, enabling flexible extension and testing.
- **Cython/Python Hybrid**: Performance-critical modules are implemented in Cython (`.pyx`, `.pxd`), while high-level logic is in Python (`.py`).
- **Event-Driven**: The engine uses an event-driven approach for processing market data, orders, and trades.
- **Testing**: The `test_kit` and `tests/` directories provide fixtures and utilities for unit, integration, and acceptance testing.
- **Configuration**: Centralized in the `config/` module, enabling environment-specific and strategy-specific settings.
- **Portability**: Designed to run on Linux, macOS, and Windows.

---

## Tips for Future Agents

- Start by exploring the `adapters/` directory for venue-specific logic, especially if integrating new exchanges or data sources.
- Use the `core/` and `model/` modules to understand the trading engine and data abstractions.
- Refer to `examples/` for practical usage patterns and quick-start scripts.
- For performance optimization, study Cython modules in `core/`, `cache/`, and `execution/`.
- Follow the coding, testing, and environment setup standards outlined in `planning/plan.md` and `docs/developer_guide/`.
- Use the `test_kit/` for writing robust tests and simulations.

---

## Further Reading

- See `planning/plan.md` for project-specific strategy and architecture.
- See `docs/developer_guide/` for detailed coding, testing, and setup standards.
- Each submodule's README or docstring (where present) provides additional context.
