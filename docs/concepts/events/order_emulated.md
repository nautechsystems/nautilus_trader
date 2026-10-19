# OrderEmulated

`OrderEmulated` records that the `OrderEmulator` has taken an order under local emulation. The
emulator applies the event to the order, updates the `Cache`, and publishes it on the `MessageBus`.

Typical transition: `INITIALIZED` -> `EMULATED`. Handler: `on_order_emulated`.

## Fields

`OrderEmulated` exposes only the
[common Python order event fields](index.md#common-python-order-event-fields).

## Example

Reading the event in a strategy handler:

```python
def on_order_emulated(self, event: OrderEmulated) -> None:
    self.log.info(f"Order {event.client_order_id} is now emulated locally")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Emulated orders](../orders/emulated.md) - The local emulation lifecycle.
