# Messaging styles

NautilusTrader is an **event-driven** framework where components communicate by sending and receiving messages.
Understanding the different messaging styles is crucial for building effective trading systems.

This guide explains the three primary messaging patterns available in NautilusTrader:

| **Messaging Style** | **Purpose** | **Best For** |
|:---|:---|:---|
| **MessageBus - Publish/Subscribe to topics** | Low-level, direct access to the message bus | Custom events, system-level communication |
| **Actor-Based - Publish/Subscribe Data** | Structured trading data exchange | Trading metrics, indicators, data needing persistence |
| **Actor-Based - Publish/Subscribe Signal** | Lightweight notifications | Simple alerts, flags, status updates |

Each approach serves different purposes and offers unique advantages. This guide will help you decide which messaging
pattern to use in your NautilusTrader applications.

## MessageBus publish/subscribe to topics

### Concept

The `MessageBus` is the central hub for all messages in NautilusTrader. It enables a **publish/subscribe** pattern
where components can publish events to **named topics**, and other components can subscribe to receive those messages.
This decouples components, allowing them to interact indirectly via the message bus.

### Key benefits and use cases

The message bus approach is ideal when you need:
- **Cross-component communication** within the system.
- **Flexibility** to define any topic and send any type of payload (any Python object).
- **Decoupling** between publishers and subscribers who don't need to know about each other.
- **Global Reach** where messages can be received by multiple subscribers.
- Working with events that don't fit within the predefined `Actor` model.
- Advanced scenarios requiring full control over messaging.

### Considerations

- You must track topic names manually (typos could result in missed messages).
- You must define handlers manually.

### Quick overview code

```python
from nautilus_trader.core.message import Event

# Define a custom event
class Each10thBarEvent(Event):
    TOPIC = "each_10th_bar"  # Topic name
    def __init__(self, bar):
        self.bar = bar

# Subscribe in a component (in Strategy)
self.msgbus.subscribe(Each10thBarEvent.TOPIC, self.on_each_10th_bar)

# Publish an event (in Strategy)
event = Each10thBarEvent(bar)
self.msgbus.publish(Each10thBarEvent.TOPIC, event)

# Handler (in Strategy)
def on_each_10th_bar(self, event: Each10thBarEvent):
    self.log.info(f"Received 10th bar: {event.bar}")
```

### Full example

[MessageBus Example](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/example_09_custom_event_with_msgbus)

## Actor-based publish/subscribe data

### Concept

This approach provides a way to exchange trading specific data between `Actor`s in the system.
(note: each `Strategy` inherits from `Actor`). It inherits from `Data`, which ensures proper timestamping
and ordering of events - crucial for correct backtest processing.

### Key Benefits and Use Cases

The Data publish/subscribe approach excels when you need:
- **Exchange of structured trading data** like market data, indicators, custom metrics, or option greeks.
- **Proper event ordering** via built-in timestamps (`ts_event`, `ts_init`) crucial for backtest accuracy.
- **Data persistence and serialization** through the `@customdataclass` decorator, integrating seamlessly with NautilusTrader's data catalog system.
- **Standardized trading data exchange** between system components.

### Considerations

- Requires defining a class that inherits from `Data` or uses `@customdataclass`.

### Inheriting from `Data` vs. using `@customdataclass`

**Inheriting from `Data` class:**
- Adds the required `ts_event` and `ts_init` attributes and their getters. These ensure proper data ordering in backtests based on timestamps.

**The `@customdataclass` decorator:**
- Adds `ts_event` and `ts_init` attributes if they are not already present.
- Provides serialization functions: `to_dict()`, `from_dict()`, `to_bytes()`, `to_arrow()`, etc.
- Enables data persistence and external communication.

### Quick overview code

```python
from nautilus_trader.core.data import Data
from nautilus_trader.model.custom import customdataclass

@customdataclass
class GreeksData(Data):
    delta: float
    gamma: float

# Publish data (in Actor / Strategy)
data = GreeksData(delta=0.75, gamma=0.1, ts_event=1630000000, ts_init=1630000000)
self.publish_data(GreeksData, data)

# Subscribe to receiving data  (in Actor / Strategy)
self.subscribe_data(GreeksData)

# Handler (this is static callback function with fixed name)
def on_data(self, data: Data):
    if isinstance(data, GreeksData):
        self.log.info(f"Delta: {data.delta}, Gamma: {data.gamma}")
```

### Full example

[Actor-Based Data Example](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/example_10_messaging_with_actor_data)

## Actor-based publish/subscribe signal

### Concept

**Signals** are a lightweight way to publish and subscribe to simple notifications within the actor framework.
This is the simplest messaging approach, requiring no custom class definitions.

### Key Benefits and Use Cases

The Signal messaging approach shines when you need:
- **Simple, lightweight notifications/alerts** like "RiskThresholdExceeded" or "TrendUp".
- **Quick, on-the-fly messaging** without defining custom classes.
- **Broadcasting alerts or flags** as primitive data (`int`, `float`, or `str`).
- **Easy API integration** with straightforward methods (`publish_signal`, `subscribe_signal`).
- **Multiple subscriber communication** where all subscribers receive signals when published.
- **Minimal setup overhead** with no class definitions required.

### Considerations

- Each signal can contain only **single value** of type: `int`, `float`, and `str`. That means no support for complex data structures or other Python types.
- In the `on_signal` handler, you can only differentiate between signals using `signal.value`, as the signal name is not accessible in the handler.

### Quick overview code

```python
# Define signal constants for better organization (optional but recommended)
import types
signals = types.SimpleNamespace()
signals.NEW_HIGHEST_PRICE = "NewHighestPriceReached"
signals.NEW_LOWEST_PRICE = "NewLowestPriceReached"

# Subscribe to signals (in Actor/Strategy)
self.subscribe_signal(signals.NEW_HIGHEST_PRICE)
self.subscribe_signal(signals.NEW_LOWEST_PRICE)

# Publish a signal (in Actor/Strategy)
self.publish_signal(
    name=signals.NEW_HIGHEST_PRICE,
    value=signals.NEW_HIGHEST_PRICE,  # value can be the same as name for simplicity
    ts_event=bar.ts_event,  # timestamp from triggering event
)

# Handler (this is static callback function with fixed name)
def on_signal(self, signal):
    # IMPORTANT: We match against signal.value, not signal.name
    match signal.value:
        case signals.NEW_HIGHEST_PRICE:
            self.log.info(
                f"New highest price was reached. | "
                f"Signal value: {signal.value} | "
                f"Signal time: {unix_nanos_to_dt(signal.ts_event)}",
                color=LogColor.GREEN
            )
        case signals.NEW_LOWEST_PRICE:
            self.log.info(
                f"New lowest price was reached. | "
                f"Signal value: {signal.value} | "
                f"Signal time: {unix_nanos_to_dt(signal.ts_event)}",
                color=LogColor.RED
            )
```

### Full example

[Actor-Based Signal Example](https://github.com/nautechsystems/nautilus_trader/tree/develop/examples/backtest/example_11_messaging_with_actor_signals)

## Summary and decision guide

Here's a quick reference to help you decide which messaging style to use:

### Decision guide: Which style to choose?

| **Use Case** | **Recommended Approach** | **Setup required** |
|:---|:---|:---|
| Custom events or system-level communication | `MessageBus` + Pub/Sub to topic | Topic + Handler management |
| Structured trading data | `Actor` + Pub/Sub Data + optional `@customdataclass` if serialization is needed | New class definition inheriting from `Data` (handler `on_data` is predefined) |
| Simple alerts/notifications | `Actor` + Pub/Sub Signal | Just signal name |
