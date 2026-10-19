# OrderReleased

`OrderReleased` records that the `OrderEmulator` has released an order after its trigger condition
was met. The emulator applies the event to the order, updates the `Cache`, and publishes it on the
`MessageBus` before routing the order onward.

Typical transition: `EMULATED` -> `RELEASED`. Handler: `on_order_released`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderReleased` carries:

| Field            | Python type | Required/default | Description                                           |
| ---------------- | ----------- | ---------------- | ----------------------------------------------------- |
| `released_price` | `Price`     | Required         | The price which released the order from the emulator. |

## Example

Reading the event in a strategy handler:

```python
def on_order_released(self, event: OrderReleased) -> None:
    self.log.info(
        f"Order {event.client_order_id} released at {event.released_price}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Emulated orders](../orders/emulated.md) - The local emulation lifecycle.
