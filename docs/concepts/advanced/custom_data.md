# Custom Data
Due to the modular nature of the Nautilus design, it is possible to set up systems 
with very flexible data streams, including custom user defined data types. This
guide covers some possible use cases for this functionality.

It's possible to create custom data types within the Nautilus system. First you
will need to define your data by subclassing from `Data`.

```{note}
As `Data` holds no state, it is not strictly necessary to call `super().__init__()`.
```

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
        The UNIX timestamp (nanoseconds) when the data event occurred.

        Returns
        -------
        int

        """
        return self._ts_event

    @property
    def ts_init(self) -> int:
        """
        The UNIX timestamp (nanoseconds) when the object was initialized.

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

```{note}
These timestamps are what allow Nautilus to correctly order data streams for backtests 
by monotonically increasing `ts_init` UNIX nanoseconds.
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
method acts as a flexible handler for all custom data.

```python
def on_data(self, data: Data) -> None:
    # First check the type of data
    if isinstance(data, MyDataPoint):
        # Do something with the data
```
