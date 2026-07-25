# OrderModifyRejected

`OrderModifyRejected` represents a `ModifyOrder` command having been rejected by the
trading venue. The `ExecutionEngine` applies it to the order, updates the `Cache`, and
publishes it on the `MessageBus`. It fires when the venue rejects a modify request.

Transition: `PENDING_UPDATE` -> previous status (for example `ACCEPTED`). Handler:
`on_order_modify_rejected`.

## Fields

Beyond the [common order event fields](index.md#common-order-event-fields), `OrderModifyRejected` carries:

| Field    | Python type | Required/default | Description                       |
| -------- | ----------- | ---------------- | --------------------------------- |
| `reason` | `str`       | Required         | The order update rejected reason. |

On this event, `venue_order_id` and `account_id` are usually populated but may be `None`,
and `reconciliation` carries a real value.

## Example

Reading the event in a strategy handler:

```python
def on_order_modify_rejected(self, event: OrderModifyRejected) -> None:
    self.log.warning(
        f"Modify rejected for {event.client_order_id}: {event.reason}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
