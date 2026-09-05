# OrderCanceled

`OrderCanceled` records an order reaching the terminal `CANCELED` state. The execution pipeline
applies it to the order, updates the `Cache`, and publishes it on the `MessageBus`. It can come from
a trading venue, simulated matching engine, local order emulator, or reconciliation. Reconciliation
can create it from a venue report or from a local timeout or missing-order policy.

Typical transitions: `PENDING_CANCEL`/`ACCEPTED` -> `CANCELED`. External and recovery paths also
allow `INITIALIZED`, `EMULATED`, `RELEASED`, `SUBMITTED`, `PENDING_UPDATE`, `TRIGGERED`, or
`PARTIALLY_FILLED` -> `CANCELED`. A re-close can append `CANCELED` while the order is already
canceled when a late fill arrived after its earlier cancellation. Handler: `on_order_canceled`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderCanceled` carries:

| Field            | Python type              | Required/default | Description                                                                    |
| ---------------- | ------------------------ | ---------------- | ------------------------------------------------------------------------------ |
| `venue_order_id` | `VenueOrderId` or `None` | `None`           | The venue-assigned order identifier, if known.                                 |
| `account_id`     | `AccountId` or `None`    | `None`           | The account associated with the order, if known.                               |
| `reconciliation` | `bool`                   | Required         | If reconciliation generated the event; this does not imply venue confirmation. |

## Example

Reading the event in a strategy handler:

```python
def on_order_canceled(self, event: OrderCanceled) -> None:
    self.log.info(f"Order {event.client_order_id} canceled")
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
- [Execution policies](../execution/policies.md#terminal-reconciliation-provenance) - Venue
  evidence and synthetic terminal policies.
