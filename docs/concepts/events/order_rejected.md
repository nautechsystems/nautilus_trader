# OrderRejected

`OrderRejected` represents an order having been rejected by the trading venue. The
`ExecutionEngine` applies it to the order, updates the `Cache`, and publishes it on the
`MessageBus`. It fires when the venue rejects the submitted order.

Typical transition: `SUBMITTED` -> `REJECTED`. Handler: `on_order_rejected`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderRejected` carries:

| Field            | Python type | Required/default | Description                                                                    |
| ---------------- | ----------- | ---------------- | ------------------------------------------------------------------------------ |
| `account_id`     | `AccountId` | Required         | The account associated with the order.                                         |
| `reason`         | `str`       | Required         | The order rejection reason.                                                    |
| `due_post_only`  | `bool`      | `False`          | If rejected because it was post-only and would execute immediately as a taker. |
| `reconciliation` | `bool`      | Required         | If generated during reconciliation.                                            |

## Example

Reading the event in a strategy handler:

```python
def on_order_rejected(self, event: OrderRejected) -> None:
    self.log.warning(f"Order {event.client_order_id} rejected: {event.reason}")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
