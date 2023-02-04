# Data
Due to the modular nature of the Nautilus design, it is possible to set up systems 
with very flexible data streams, including custom user defined data types. This
guide covers some possible use cases for this functionality.

## Custom/Generic Data
It's possible to create custom data types within the Nautilus system. First you
will need to define your data by subclassing from `Data`.

```python
from nautilus_trader.core.data import Data


class MyDataPoint(Data):

    def __init__(
        self,
        label: str,
        x: int,
        y: int,
        z: int,
        ts_event: int,
        ts_init: int,
    ) -> None:
        super().__init__(ts_event, ts_init)

        self.label = label
        self.x = x
        self.y = y
        self.z = z
```

As you can see, this requires you to call the `Data` base classes `__init__` method, 
and passing in the UNIX **nanosecond** timestamps for the event, and object initialization.

```{note}
Passing these timestamps is what allows Nautilus to correctly order data streams
by monotonically increasing `ts_init` timestamps for backtests.
```

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
method acts as a flexible handler for all custom/generic data.

```python
def on_data(self, data: Data):
    # First check the type of data
    if isinstance(data, MyDataPoint):
        # Do something with the data
```
