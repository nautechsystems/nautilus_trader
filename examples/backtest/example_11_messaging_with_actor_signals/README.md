# Actor-Based Signal Messaging Example

This example demonstrates the simplest form of messaging in NautilusTrader using *actor-based signals*.
It shows how to implement lightweight notifications between components using *string-based signals*.

## What You'll Learn

- How to use signals for simple notifications (price extremes in this case).
- How to publish signals with single string values.
- How to subscribe to signals and handle them in `on_signal` callback.

## Implementation Highlights

- Uses `SimpleNamespace` for signal name constants.
- Shows both signal publishing and subscription.
- Demonstrates signal handling with pattern matching in `on_signal`.
