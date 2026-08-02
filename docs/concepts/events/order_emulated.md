# OrderEmulated

`OrderEmulated` represents an order having become emulated by the Nautilus system. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the `OrderEmulator` takes an order under local emulation.

Transition: `INITIALIZED` -> `EMULATED`. Handler: `on_order_emulated`.

## Fields

`OrderEmulated` carries only the [common order event fields](index.md#common-order-event-fields). On this event, `venue_order_id` and `account_id` are
both `None`, `reconciliation` is always `False`, and `ts_event` equals `ts_init`.

## Example

Reading the event in a strategy handler:

```python
def on_order_emulated(self, event: OrderEmulated) -> None:
    self.log.info(f"Order {event.client_order_id} is now emulated locally")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Emulated orders](../orders/emulated.md) - The local emulation lifecycle.
