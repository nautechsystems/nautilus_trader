# Roadmap

This document outlines the key priorities and upcoming goals for **NautilusTrader**,
charting its path as a cutting-edge platform for high-performance algorithmic trading.

Given the dynamic nature of the project, priorities may evolve to keep pace with the fast-moving development cycle.
For real-time updates and detailed task tracking, refer to the [NautilusTrader Kanban board](https://github.com/orgs/nautechsystems/projects/3).

**Note**: Bug fixes and roadmap priorities take precedence over feature requests to ensure stability
and progress. However, pull requests (PRs) for improvements and new features are always welcome.
For more details, see the [CONTRIBUTING.md](/CONTRIBUTING.md).

## Vision

To establish NautilusTrader as the standard platform for quantitative algorithmic trading, combining
performance, reliability, usability, and comprehensive documentation for traders and developers alike.

## Priorities

1. **Port core to Rust**

   **Goal**: Leverage Rust's performance and safety features to improve reliability, performance and scalability.
   - Rewrite performance-critical components in Rust (replacing existing Cython modules).
   - Ensure interoperability between Rust and Python layers using PyO3.
   - Benchmark performance improvements throughout the transition.

2. **Improve Documentation and Tutorials**

    **Goal**: Lower the learning curve for new users and empower developers with clear, comprehensive guides:
   - Fill gaps in user and developer documentation by adding missing sections.
   - Add additional tutorials and examples.

3. **Improve Code Ergonomics**

    **Goal**: Simplify the development experience for users and contributors:
   - Enhance type annotations and support for Python import resolution.
   - Standardize naming conventions and refine APIs for greater intuitiveness.
   - Streamline configuration and setup processes to minimize friction.
   - Refactor modules and namespaces to improve readability and maintainability.

## Additional Enhancements

As we progress on the top priorities, we also plan to focus on the following enhancements:

- Expand integrations with adapters to support trading venues and data providers.
- Enhance the backtesting engine with additional features.
- Enhance order book execution dynamics with additional features, including user order interactions, persistent book changes, and expanded microstructure simulations.

## NautilusTrader v2.0 and Beyond

- **Achieving Stable Status**: While NautilusTrader is already successfully used in production, v2.0 represents a significant milestone toward establishing a stable API.
- **Focus Areas**: The v2.0 initiative will prioritize API consistency, long-term maintainability, and meeting the rigorous standards of live trading environments.
- **Formal Deprecations**: v2.0 will introduce formal deprecations, making it easier to adopt changes and new features while maintaining clarity for developers.
- **Python API Commitment**: Despite transitioning the core to Rust, NautilusTrader will continue to provide a user-facing Python API.

## Charting the Future

This roadmap builds on NautilusTrader’s strong foundation, driving continuous refinement while
expanding its possibilities and capabilities for algorithmic traders and developers.
