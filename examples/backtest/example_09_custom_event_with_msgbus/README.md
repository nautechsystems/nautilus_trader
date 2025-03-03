# Example: Custom events and Message Bus Publish/Subscribe Pattern

This example demonstrates:
* how to create and use custom events with the message bus in a **NautilusTrader** strategy.
* how to create a custom event, publish it when specific conditions are met and subscribe to handle these events.

This pattern is not only useful for proper event-driven communication between different parts of your trading system,
but also for generating and receiving events within the same Actor/Strategy.

This self-communication pattern through the message bus provides a clean and consistent way to handle
state changes or conditional notifications within your strategy.

**What this example demonstrates:**

- Creating custom events using Python dataclasses
- Publishing events using the message bus
- Subscribing to events using the message bus
- Handling received events in the strategy
- Using events for condition-based notifications (every 10th bar in this case)
