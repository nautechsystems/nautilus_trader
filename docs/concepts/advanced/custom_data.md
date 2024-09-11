# Custom Data
Due to the modular nature of the Nautilus design, it is possible to set up systems 
with very flexible data streams, including custom user-defined data types. This
guide covers some possible use cases for this functionality.

It's possible to create custom data types within the Nautilus system. First you
will need to define your data by subclassing from `Data`.

:::info
As `Data` holds no state, it is not strictly necessary to call `super().__init__()`.
:::

```python
from nautilus_trader.core.data import Data


class MyDataPoint(Data):
    """
    This is an example of a user-defined data class, inheriting from the base class `Data`.

    The fields `label`, `x`, `y`, and `z` in this class are examples of arbitrary user data.
    """

    def __init__(
        self,
        label: str,
        x: int,
        y: int,
        z: int,
        ts_event: int,
        ts_init: int,
    ) -> None:
        self.label = label
        self.x = x
        self.y = y
        self.z = z
        self._ts_event = ts_event
        self._ts_init = ts_init

    @property
    def ts_event(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        UNIX timestamp (nanoseconds) when the object was initialized.

        Returns
        -------
        int

        """
        return self._ts_init

```

The `Data` abstract base class acts as a contract within the system and requires two properties 
for all types of data: `ts_event` and `ts_init`. These represent the UNIX nanosecond timestamps 
for when the event occurred and when the object was initialized, respectively.

The recommended approach to satisfy the contract is to assign `ts_event` and `ts_init` 
to backing fields, and then implement the `@property` for each as shown above 
(for completeness, the docstrings are copied from the `Data` base class).

:::info
These timestamps enable Nautilus to correctly order data streams for backtests
using monotonically increasing `ts_init` UNIX nanoseconds.
:::

We can now work with this data type for backtesting and live trading. For instance,
we could now create an adapter which is able to parse and create objects of this
type - and send them back to the `DataEngine` for consumption by subscribers.

You can subscribe to these custom data types within your actor/strategy in the 
following way:

```python
self.subscribe_data(
    data_type=DataType(MyDataPoint, metadata={"some_optional_category": 1}),
    client_id=ClientId("MY_ADAPTER"),
)
```

This will result in your actor/strategy passing these received `MyDataPoint` 
objects to your `on_data` method. You will need to check the type, as this 
method acts as a flexible handler for all custom data.

```python
def on_data(self, data: Data) -> None:
    # First check the type of data
    if isinstance(data, MyDataPoint):
        # Do something with the data
```

### Publishing and receiving signal data

Here is an example of publishing and receiving signal data using the `MessageBus` from an actor or strategy. 
A signal is an automatically generated custom data identified by a name containing only one value of a basic type 
(str, float, int, bool or bytes).

```python
self.publish_signal("signal_name", value, ts_event)
self.subscribe_signal("signal_name")

def on_data(self, data):
    if data.is_signal("signal_name"):
        print("Signal", data)
```

## Option Greeks example

This example demonstrates how to create a custom data type for option Greeks, specifically the delta.
By following these steps, you can create custom data types, subscribe to them, publish them, and store
them in the `Cache` or `ParquetDataCatalog` for efficient retrieval.

```python
import msgspec
from nautilus_trader.core.data import Data
from nautilus_trader.model.data import DataType
from nautilus_trader.serialization.base import register_serializable_type
from nautilus_trader.serialization.arrow.serializer import register_arrow
import pyarrow as pa

from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.core.datetime import dt_to_unix_nanos, unix_nanos_to_dt, format_iso8601


def unix_nanos_to_str(unix_nanos):
    return format_iso8601(unix_nanos_to_dt(unix_nanos))


class GreeksData(Data):
    def __init__(
        self, instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX"),
        ts_event: int = 0,
        ts_init: int = 0,
        delta: float = 0.0,
    ) -> None:
        self.instrument_id = instrument_id
        self._ts_event = ts_event
        self._ts_init = ts_init
        self.delta = delta

    def __repr__(self):
        return (f"GreeksData(ts_init={unix_nanos_to_str(self._ts_init)}, instrument_id={self.instrument_id}, delta={self.delta:.2f})")

    @property
    def ts_event(self):
        return self._ts_event

    @property
    def ts_init(self):
        return self._ts_init

    def to_dict(self):
        return {
            "instrument_id": self.instrument_id.value,
            "ts_event": self._ts_event,
            "ts_init": self._ts_init,
            "delta": self.delta,
        }

    @classmethod
    def from_dict(cls, data: dict):
        return GreeksData(InstrumentId.from_str(data["instrument_id"]), data["ts_event"], data["ts_init"], data["delta"])

    def to_bytes(self):
        return msgspec.msgpack.encode(self.to_dict())

    @classmethod
    def from_bytes(cls, data: bytes):
        return cls.from_dict(msgspec.msgpack.decode(data))

    def to_catalog(self):
        return pa.RecordBatch.from_pylist([self.to_dict()], schema=GreeksData.schema())

    @classmethod
    def from_catalog(cls, table: pa.Table):
        return [GreeksData.from_dict(d) for d in table.to_pylist()]

    @classmethod
    def schema(cls):
        return pa.schema(
            {
                "instrument_id": pa.string(),
                "ts_event": pa.int64(),
                "ts_init": pa.int64(),
                "delta": pa.float64(),
            }
        )
```

### Publishing and receiving data

Here is an example of publishing and receiving data using the `MessageBus` from an actor or strategy:

```python
register_serializable_type(GreeksData, GreeksData.to_dict, GreeksData.from_dict)

def publish_greeks(self, greeks_data: GreeksData):
    self.publish_data(DataType(GreeksData), greeks_data)

def subscribe_to_greeks(self):
    self.subscribe_data(DataType(GreeksData))

def on_data(self, data):
    if isinstance(GreeksData):
        print("Data", data)
```

### Writing and reading data using the cache

Here is an example of writing and reading data using the `Cache` from an actor or strategy:

```python
def greeks_key(instrument_id: InstrumentId):
    return f"{instrument_id}_GREEKS"

def cache_greeks(self, greeks_data: GreeksData):
    self.cache.add(greeks_key(greeks_data.instrument_id), greeks_data.to_bytes())

def greeks_from_cache(self, instrument_id: InstrumentId):
    return GreeksData.from_bytes(self.cache.get(greeks_key(instrument_id)))
```

### Writing and reading data using a catalog

For streaming custom data to feather files or writing it to parquet files in a catalog 
(`register_arrow` needs to be used):

```python
register_arrow(GreeksData, GreeksData.schema(), GreeksData.to_catalog, GreeksData.from_catalog)

from nautilus_trader.persistence.catalog import ParquetDataCatalog
catalog = ParquetDataCatalog('.')

catalog.write_data([GreeksData()])
```

## Creating a custom data class automatically

The `@customdataclass` decorator enables the creation of a custom data class with default
implementations for all the features described above.

Each method can also be overridden if needed. Here is an example of its usage:

```python
from nautilus_trader.model.custom import customdataclass


@customdataclass
class GreeksTestData(Data):
    instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX")
    delta: float = 0.0


GreeksTestData(
    instrument_id=InstrumentId.from_str("CL.GLBX"),
    delta=1000.0,
    ts_event=1,
    ts_init=2,
)
```

### Custom data type stub

To enhance development convenience and improve code suggestions in your IDE, you can create a `.pyi`
stub file with the proper constructor signature for your custom data types as well as type hints for attributes. 
This is particularly useful when the constructor is dynamically generated at runtime, as it allows the IDE to recognize 
and provide suggestions for the class's methods and attributes.

For instance, if you have a custom data class defined in `greeks.py`, you can create a corresponding `greeks.pyi` file 
with the following constructor signature:

```python
from nautilus_trader.core.data import Data
from nautilus_trader.model.identifiers import InstrumentId


class GreeksData(Data):
    instrument_id: InstrumentId
    delta: float
    
    def __init__(
        self,
        ts_event: int = 0,
        ts_init: int = 0,
        instrument_id: InstrumentId = InstrumentId.from_str("ES.GLBX"),
        delta: float = 0.0,
  ) -> GreeksData: ...
```
