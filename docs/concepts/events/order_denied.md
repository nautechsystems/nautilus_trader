# OrderDenied

`OrderDenied` represents an order having been denied by the Nautilus system. The execution pipeline
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. It fires when an
otherwise valid order cannot be submitted, for example due to a risk limit or an unsupported
feature. The risk engine, execution engine, execution algorithms, and execution clients can all deny
an order.

Typical transition: `INITIALIZED` -> `DENIED`. Handler: `on_order_denied`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderDenied` carries:

| Field    | Python type | Required/default | Description                                                              |
| -------- | ----------- | ---------------- | ------------------------------------------------------------------------ |
| `reason` | `str`       | Required         | The standardized denied reason code, with an optional diagnostic suffix. |

## Example

Reading the event in a strategy handler:

```python
def on_order_denied(self, event: OrderDenied) -> None:
    self.log.warning(f"Order {event.client_order_id} denied: {event.reason}")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Execution](../execution/) - Risk checks and the pre-trade pipeline.
- [Order denied reasons](../execution/index.md#order-denied-reasons) - The standardized code set and
  message forms.
- [Orders](../orders/) - Order types and the state machine.
