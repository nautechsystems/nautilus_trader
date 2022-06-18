# Advanced Orders

The following guide should be read in conjunction with the specific documentation from the broker or exchange 
involving these order types, lists/groups and execution instructions (such as for Interactive Brokers).

## Order Lists
Larger order bulks can be grouped together into a list with a common `order_list_id`.
The orders contained in this bulk may or may not have a contingent relationship with
each other, as this is specific to how the orders themselves are constructed, and the
specific exchange they are being routed to.

## Contingency Types

- `OTO` are parent orders with 'one-triggers-other' child orders.
- `OCO` are linked orders with `linked_order_ids` which are contingent on the others remaining quantity (one-cancels/reduces-other).


## Bracket Orders

Is a group of orders including some entry order bracketed by two child orders being a take-profit `LIMIT` order and stop-loss `STOP_MARKET` order.
The best way to build this group is via the [OrderFactory](https://docs.nautilustrader.io/api_reference/common.html#module-nautilus_trader.common.factories).
