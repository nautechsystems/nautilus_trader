# Message Bus

The `MessageBus` is a fundamental component of the platform, facilitating communicate between 
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

The `MessageBus` can be 'backed' with any database or message broker technology which has an 
integration written for it, this then allows external publishing of messages.

```{note}
Currently Redis is supported for all serializable messages which are published.
```

Under the hood, when a backing database (or any other compatible technology) is configured,
all outgoing messages are first serialized. These serialized messages are then transmitted via a 
Multiple-Producer Single-Consumer (MPSC) channel to a separate thread, which is implemented in Rust. 
In this separate thread, the message is written to its final destination, which is presently Redis streams.

This design is primarily driven by performance considerations. By offloading the I/O operations to a separate thread, 
we ensure that the main thread remains unblocked and can continue its tasks without being hindered by the potentially
time-consuming operations involved in interacting with a database or client.

### Serialization

Most built in objects are serializable, as well as primitives such as `str`, `int`, `float`, `bool` and `bytes`.
Additional custom types can be registered by calling the following registration function from the `serialization` subpackage:

```python
def register_serializable_object(
    obj,
    to_dict: Callable[[Any], dict[str, Any]],
    from_dict: Callable[[dict[str, Any]], Any],
):
    """
    Register the given object with the global serialization object maps.

    Parameters
    ----------
    obj : object
        The object to register.
    to_dict : Callable[[Any], dict[str, Any]]
        The delegate to instantiate a dict of primitive types from the object.
    from_dict : Callable[[dict[str, Any]], Any]
        The delegate to instantiate the object from a dict of primitive types.
    """
    ...
```
## Configuration

The message bus backing can be configured by importing the `MessageBusConfig` object and passing this to
your `TradingNodeConfig`. Each of these config options will be described below.

```python
...  # Other config omitted
message_bus=MessageBusConfig(
    database=DatabaseConfig(),
    encoding="json",
    stream="streams",
    use_instance_id=False,
    timestamps_as_iso8601=True,
    types_filter=[QuoteTick, TradeTick],
    autotrim_mins=30,
)
...
```

```python

class MessageBusConfig(NautilusConfig, frozen=True):
    """
    Configuration for ``MessageBus`` instances.

    Parameters
    ----------
    database : DatabaseConfig, optional
        The configuration for the message bus backing database.
    stream : str, optional
        The additional prefix for externally published stream names (must have a `database` config).
    use_instance_id : bool, default False
        If the traders instance ID should be used in stream names.
    encoding : str, {'msgpack', 'json'}, default 'msgpack'
        The encoding for database operations, controls the type of serializer used.
    timestamps_as_iso8601, default False
        If timestamps should be persisted as ISO 8601 strings.
        If `False` then will persit as UNIX nanoseconds.
    types_filter : list[type], optional
        A list of serializable types *not* to publish externally.
    autotrim_mins : int, optional
        The lookback window in minutes for automatic stream trimming.
        The actual window may extend up to one minute beyond the specified value since streams are
        trimmed at most once every minute.
        Note that this feature requires Redis version 6.2.0 or higher; otherwise it will result
        in acommand syntax error.

    """

    database: DatabaseConfig | None = None
    stream: str | None = None
    use_instance_id: bool = False
    encoding: str = "msgpack"
    timestamps_as_iso8601: bool = False
    types_filter: list[type] | None = None
    autotrim_mins: int | None = None

```

### Database config
A `DatabaseConfig` must be provided, for a default Redis setup on the local
loopback, you can simple pass a `DatabaseConfig()` which will use defaults to match.

### Trader keys

Trader keys are essential for identifying individual trader nodes and organizing messages within streams.
They can be tailored to meet your specific requirements and use cases. In the context of message bus streams, a trader key is typically structured as follows:

```
{stream}:{trader_id}:{instance_id}
```

The following options are available for configuring trader keys:

#### Stream
The `stream` string allows you to group all streams for a single trader instance, or organize messages related to a group of trader instances.
By configuring this grouping behavior, pass a string to the `stream` configuration option.

#### Instance ID

Each trader node is assigned a unique 'instance ID,' which is a UUIDv4. This instance ID helps distinguish individual traders when messages 
are distributed across multiple streams. You can include the instance ID in the trader key by setting the `use_instance_id` configuration option to `True`.
This is particularly useful when you need to track and identify traders across various streams in a multi-node trading system.

### Types filtering

When messages are published on the message bus, they are serialized and written to a stream, provided that a backing for the message bus has been configured and enabled.
However, in some cases, you may want to filter out certain types of messages from being externally published to prevent the stream from being 'flooded' with data, 
such as quotes or other high-frequency information.

To enable this filtering mechanism, pass a list of `type` objects to the `types_filter` parameter in the message bus configuration, specifying which types of messages should be excluded from external publication.

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

```{note}
The current Redis implementation will maintain the `autotrim_mins` as a maximum width (plus roughly a minute, as streams are trimmed no more than once per minute).
Rather than for instance a maximum lookback window based on the current wall clock time.
```
