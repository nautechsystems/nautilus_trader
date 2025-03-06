# Example - Messaging with Actor Data

This example demonstrates how to work with custom data classes
and the Actor's publish/subscribe mechanism in NautilusTrader.

## What You'll Learn

- How to create custom data classes (both serializable and non-serializable).
- How to publish and subscribe to custom data using Actor methods.
- How to handle custom data events in your strategy.

## Implementation Details

The strategy showcases two approaches to custom data classes:

- `Last10BarsStats`: A simple non-serializable data.
- `Last10BarsStatsSerializable`: A serializable data showing proper setup for data persistence and transfer between nodes.
