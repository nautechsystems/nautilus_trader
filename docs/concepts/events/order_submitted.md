# OrderSubmitted

`OrderSubmitted` represents an order having been submitted by the system to the trading
venue. The `ExecutionEngine` applies it to the order, updates the `Cache`, and publishes
it on the `MessageBus`. It fires when the system sends the order to the venue and awaits
acknowledgement.

Typical transition: `INITIALIZED` / `RELEASED` -> `SUBMITTED`. Handler: `on_order_submitted`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderSubmitted` carries:

| Field        | Python type | Required/default | Description                            |
| ------------ | ----------- | ---------------- | -------------------------------------- |
| `account_id` | `AccountId` | Required         | The account associated with the order. |

## Example

Reading the event in a strategy handler:

```python
def on_order_submitted(self, event: OrderSubmitted) -> None:
    self.log.info(f"Order {event.client_order_id} submitted ({event.account_id})")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
