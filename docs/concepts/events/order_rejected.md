# OrderRejected

`OrderRejected` records an order reaching the terminal `REJECTED` state. The `ExecutionEngine`
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. It normally
comes from an explicit venue rejection. Reconciliation can also create it from a venue report or
after a local timeout or missing-order policy expires.

Typical transition: `SUBMITTED` -> `REJECTED`. External and reconciliation paths also allow
`INITIALIZED`, `ACCEPTED`, `PENDING_UPDATE`, `PENDING_CANCEL`, or `TRIGGERED` -> `REJECTED`.
Handler: `on_order_rejected`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderRejected` carries:

| Field            | Python type | Required/default | Description                                                                    |
| ---------------- | ----------- | ---------------- | ------------------------------------------------------------------------------ |
| `account_id`     | `AccountId` | Required         | The account associated with the order.                                         |
| `reason`         | `str`       | Required         | The venue reason or local reconciliation policy reason.                        |
| `due_post_only`  | `bool`      | `False`          | If rejected because it was post-only and would execute immediately as a taker. |
| `reconciliation` | `bool`      | Required         | If reconciliation generated the event; this does not imply venue confirmation. |

## Example

Reading the event in a strategy handler:

```python
def on_order_rejected(self, event: OrderRejected) -> None:
    self.log.warning(f"Order {event.client_order_id} rejected: {event.reason}")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
- [Execution policies](../execution/policies.md#terminal-reconciliation-provenance) - Venue
  evidence and synthetic terminal policies.
