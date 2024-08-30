# Message Bus

The `MessageBus` is a fundamental part of the platform, facilitating communicate between 
various system components through message passing. This approach enables a loosely coupled architecture,
where components can interact without strong dependencies. Messages exchanged via the message bus 
can be categorized into three distinct types:

- Data
- Events
- Commands

## Data and signal publishing

While the message bus is considered a lower-level component, users typically interact with it indirectly.
`Actor` and `Strategy` classes provide two convenience methods built on top of the underlying `MessageBus`, for
easier custom data and signal publishing:

```python
def publish_data(self, data_type: DataType, data: Data) -> None:
def publish_signal(self, name: str, value, ts_event: int | None = None) -> None:
```

## Direct access

For advanced users and specific use cases, direct access to the message bus is available from within `Actor` and `Strategy` 
classes through the `self.msgbus` reference, which exposes the message bus interface directly.
To publish a custom message, simply provide a topic as a `str` and any Python `object` as the message payload, for example:

```python

self.msgbus.publish("MyTopic", "MyMessage")
```

## External publishing

The `MessageBus` can be *backed* with any database or message broker technology which has an
integration written for it, this then enables external publishing of messages.

:::info
Redis is currently supported for all serializable messages which are published externally.
The minimum supported Redis version is 6.2.0.
:::

Under the hood, when a backing database (or any other compatible technology) is configured,
all outgoing messages are first serialized. These serialized messages are then transmitted via a 
Multiple-Producer Single-Consumer (MPSC) channel to a separate thread, which is implemented in Rust. 
In this separate thread, the message is written to its final destination, which is presently Redis streams.

This design is primarily driven by performance considerations. By offloading the I/O operations to a separate thread, 
we ensure that the main thread remains unblocked and can continue its tasks without being hindered by the potentially
time-consuming operations involved in interacting with a database or client.

### Serialization

Most Nautilus built-in objects are serializable, dictionaries `dict[str, Any]` containing serializable primitives, as well as primitive types themselves such as `str`, `int`, `float`, `bool` and `bytes`.
Additional custom types can be registered by calling the following registration function from the `serialization` subpackage:

```python
def register_serializable_type(
    cls,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
):
    ...
```

- `cls`: The type to register
- `to_dict`: The delegate to instantiate a dict of primitive types from the object
- `from_dict`: The delegate to instantiate the object from a dict of primitive types

## Configuration

The message bus external backing technology can be configured by importing the `MessageBusConfig` object and passing this to
your `TradingNodeConfig`. Each of these config options will be described below.

```python
...  # Other config omitted
message_bus=MessageBusConfig(
    database=DatabaseConfig(),
    encoding="json",
    timestamps_as_iso8601=True,
    buffer_interval_ms=100,
    autotrim_mins=30,
    use_trader_prefix=True,
    use_trader_id=True,
    use_instance_id=False,
    streams_prefix="streams",
    types_filter=[QuoteTick, TradeTick],
)
...
```

### Database config
A `DatabaseConfig` must be provided, for a default Redis setup on the local
loopback you can pass a `DatabaseConfig()`, which will use defaults to match.

### Encoding

Two encodings are currently supported by the built-in `Serializer` used by the `MessageBus`:
- JSON (`json`)
- MessagePack (`msgpack`)

Use the `encoding` config option to control the message writing encoding.

:::tip
The `msgpack` encoding is used by default as it offers the most optimal serialization and memory performance.
It's recommended to use `json` encoding for human readability when performance is not a primary concern.
:::

### Timestamp formatting

By default timestamps are formatted as UNIX epoch nanosecond integers. Alternatively you can
configure ISO 8601 string formatting by setting the `timestamps_as_iso8601` to `True`.

### Message stream keys

Message stream keys are essential for identifying individual trader nodes and organizing messages within streams.
They can be tailored to meet your specific requirements and use cases. In the context of message bus streams, a trader key is typically structured as follows:

```
trader:{trader_id}:{instance_id}:{streams_prefix}
```

The following options are available for configuring message stream keys:

#### Trader prefix

If the key should begin with the `trader` string.

#### Trader ID

If the key should include the trader ID for the node.

#### Instance ID

Each trader node is assigned a unique 'instance ID,' which is a UUIDv4. This instance ID helps distinguish individual traders when messages 
are distributed across multiple streams. You can include the instance ID in the trader key by setting the `use_instance_id` configuration option to `True`.
This is particularly useful when you need to track and identify traders across various streams in a multi-node trading system.

#### Streams prefix

The `streams_prefix` string enables you to group all streams for a single trader instance or organize
messages for multiple instances. Configure this by passing a string to the `streams_prefix` configuration 
option, ensuring other prefixes are set to false.

#### Stream per topic

Indicates whether the producer will write a separate stream for each topic. This is particularly 
useful for Redis backings, which do not support wildcard topics when listening to streams. 
If set to False, all messages will be written to the same stream.

:::info
Redis does not support wildcard stream topics. For better compatibility with Redis, it is recommended to set this option to False.
:::

### Types filtering

When messages are published on the message bus, they are serialized and written to a stream if a backing
for the message bus is configured and enabled. To prevent flooding the stream with data like high-frequency 
quotes, you may filter out certain types of messages from external publication.

To enable this filtering mechanism, pass a list of `type` objects to the `types_filter` parameter in the message bus configuration,
specifying which types of messages should be excluded from external publication.

```python
from nautilus_trader.config import MessageBusConfig
from nautilus_trader.data import TradeTick
from nautilus_trader.data import QuoteTick

# Create a MessageBusConfig instance with types filtering
message_bus = MessageBusConfig(
    types_filter=[QuoteTick, TradeTick]
)

```

### Stream auto-trimming

The `autotrim_mins` configuration parameter allows you to specify the lookback window in minutes for automatic stream trimming in your message streams.
Automatic stream trimming helps manage the size of your message streams by removing older messages, ensuring that the streams remain manageable in terms of storage and performance.

:::info
The current Redis implementation will maintain the `autotrim_mins` as a maximum width (plus roughly a minute, as streams are trimmed no more than once per minute).
Rather than a maximum lookback window based on the current wall clock time.
:::

The minimum supported Redis version is 6.2.0.

## External streams

The message bus within a `TradingNode` (node) is referred to as the "internal message bus".
A producer node is one which publishes messages onto an external stream (see [external publishing](#external-publishing)).
The consumer node listens to external streams to receive and publish deserialized message payloads on its internal message bus.
                                                                         
                      ┌───────────────────────────┐                      
                      │                           │                      
                      │                           │                      
                      │                           │                      
                      │      Producer Node        │                      
                      │                           │                      
                      │                           │                      
                      │                           │                      
                      │                           │                      
                      │                           │                      
                      │                           │                      
                      └─────────────┬─────────────┘                      
                                    │                                    
                                    │                                    
    ┌───────────────────────────────▼──────────────────────────────┐     
    │                                                              │     
    │                            Stream                            │     
    │                                                              │     
    └─────────────┬────────────────────────────────────┬───────────┘     
                  │                                    │                 
                  │                                    │                 
    ┌─────────────▼───────────┐          ┌─────────────▼───────────┐     
    │                         │          │                         │     
    │                         │          │                         │     
    │     Consumer Node 1     │          │     Consumer Node 2     │     
    │                         │          │                         │     
    │                         │          │                         │     
    │                         │          │                         │     
    │                         │          │                         │     
    │                         │          │                         │     
    │                         │          │                         │     
    │                         │          │                         │     
    └─────────────────────────┘          └─────────────────────────┘     

:::tip
Set the `LiveDataEngineConfig.external_clients` with the list of `client_id`s intended to represent the external streaming clients.
The `DataEngine` will filter out subscription commands for these clients, ensuring that the external streaming provides the necessary data for any subscriptions to these clients.
:::

### Example configuration

The following example details a streaming setup where a producer node publishes Binance data externally,
and a downstream consumer node publishes these data messages onto its internal message bus.

#### Producer node

We configure the `MessageBus` of the producer node to publish to a `"binance"` stream.
The settings `use_trader_id`, `use_trader_prefix`, and `use_instance_id` are all set to `False`
to ensure a simple and predictable stream key that the consumer nodes can register for.

```python
    message_bus=MessageBusConfig(
        database=DatabaseConfig(timeout=2),
        use_trader_id=False,
        use_trader_prefix=False,
        use_instance_id=False,
        streams_prefix="binance",  # <---
        stream_per_topic=False,
        autotrim_mins=30,
    ),
```

#### Consumer node

We configure the `MessageBus` of the consumer node to receive messages from the same `"binance"` stream.
The node will listen to the external stream keys to publish these messages onto its internal message bus.
Additionally, we declare the client ID `"BINANCE_EXT"` as an external client. This ensures that the 
`DataEngine` does not attempt to send data commands to this client ID, as we expect these messages to be
published onto the internal message bus from the external stream, to which the node has subscribed to the relevant topics.

```python
    data_engine=LiveDataEngineConfig(
        external_clients=[ClientId("BINANCE_EXT")],
    ),
    message_bus=MessageBusConfig(
        database=DatabaseConfig(timeout=2),
        external_streams=["binance"],  # <---
    ),
```
