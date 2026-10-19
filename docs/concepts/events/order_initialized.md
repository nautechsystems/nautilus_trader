# OrderInitialized

`OrderInitialized` is the seed event for a new order. It carries enough information to send the
order over the wire and reconstruct it with the same properties. The execution pipeline stores the
order in the `Cache` and publishes the event on the `MessageBus`.

The event seeds both locally created orders and external orders materialized during reconciliation.
Handler: `on_order_initialized`.

## Fields

Beyond the [common Python order event fields](index.md#common-python-order-event-fields),
`OrderInitialized` carries:

| Field                   | Python type                     | Required/default | Description                                         |
| ----------------------- | ------------------------------- | ---------------- | --------------------------------------------------- |
| `order_side`            | `OrderSide`                     | Required         | The order side.                                     |
| `order_type`            | `OrderType`                     | Required         | The order type.                                     |
| `quantity`              | `Quantity`                      | Required         | The order quantity.                                 |
| `time_in_force`         | `TimeInForce`                   | Required         | The order time in force.                            |
| `post_only`             | `bool`                          | Required         | If the order only provides liquidity.               |
| `reduce_only`           | `bool`                          | Required         | If the order carries the reduce-only instruction.   |
| `quote_quantity`        | `bool`                          | Required         | If quantity is denominated in the quote currency.   |
| `reconciliation`        | `bool`                          | Required         | If the event was generated during reconciliation.   |
| `price`                 | `Price` or `None`               | `None`           | The limit price.                                    |
| `activation_price`      | `Price` or `None`               | `None`           | The activation price for a trailing-stop order.     |
| `trigger_price`         | `Price` or `None`               | `None`           | The stop trigger price.                             |
| `trigger_type`          | `TriggerType` or `None`         | `None`           | The trigger type.                                   |
| `limit_offset`          | `Decimal` or `None`             | `None`           | The trailing offset for the limit price.            |
| `trailing_offset`       | `Decimal` or `None`             | `None`           | The trailing offset for the trigger price.          |
| `trailing_offset_type`  | `TrailingOffsetType` or `None`  | `None`           | The trailing offset type.                           |
| `expire_time`           | `int` or `None`                 | `None`           | The UNIX expiration timestamp in nanoseconds.       |
| `display_qty`           | `Quantity` or `None`            | `None`           | The quantity displayed on the public book.          |
| `emulation_trigger`     | `TriggerType` or `None`         | `None`           | The market price trigger for local emulation.       |
| `trigger_instrument_id` | `InstrumentId` or `None`        | `None`           | The instrument that supplies the emulation trigger. |
| `contingency_type`      | `ContingencyType` or `None`     | `None`           | The order contingency type.                         |
| `order_list_id`         | `OrderListId` or `None`         | `None`           | The associated order list ID.                       |
| `linked_order_ids`      | `list[ClientOrderId]` or `None` | `None`           | The linked client order IDs.                        |
| `parent_order_id`       | `ClientOrderId` or `None`       | `None`           | The parent client order ID.                         |
| `exec_algorithm_id`     | `ExecAlgorithmId` or `None`     | `None`           | The execution algorithm ID.                         |
| `exec_algorithm_params` | `dict[str, str]` or `None`      | `None`           | The execution algorithm parameters.                 |
| `exec_spawn_id`         | `ClientOrderId` or `None`       | `None`           | The spawning primary client order ID.               |
| `tags`                  | `list[str]` or `None`           | `None`           | Custom user tags.                                   |

## Example

Reading the event in a strategy handler:

```python
def on_order_initialized(self, event: OrderInitialized) -> None:
    self.log.info(
        f"Initialized {event.order_type} {event.order_side} {event.quantity} {event.instrument_id}",
    )
```

## Related guides

- [Events](index.md) - Event categories, dispatch, and the common order event fields.
- [Orders](../orders/) - Order types and the state machine.
