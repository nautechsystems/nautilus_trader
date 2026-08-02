# OrderReleased

`OrderReleased` represents an order having been released from the `OrderEmulator`. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the emulator's trigger price condition is met and the order is
released to the venue.

Transition: `EMULATED` -> `RELEASED`. Handler: `on_order_released`.

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderReleased` carries:

| Field            | Python type | Required/default | Description                                           |
| ---------------- | ----------- | ---------------- | ----------------------------------------------------- |
| `released_price` | `Price`     | Required         | The price which released the order from the emulator. |

On this event, `venue_order_id` and `account_id` are both `None`, `reconciliation` is
always `False`, and `ts_event` equals `ts_init`.

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
