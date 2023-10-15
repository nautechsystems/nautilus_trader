# Advanced Orders

The following guide should be read in conjunction with the specific documentation from the broker or exchange 
involving these order types, lists/groups and execution instructions (such as for Interactive Brokers).

## Order Lists
Combinations of contingent orders, or larger order bulks can be grouped together into a list with a common 
`order_list_id`. The orders contained in this list may or may not have a contingent relationship with
each other, as this is specific to how the orders themselves are constructed, and the
specific exchange they are being routed to.

## Contingency Types

- `OTO` are parent orders with 'one-triggers-other' child orders.
- `OCO` are linked orders with `linked_order_ids` which are contingent on the other(s) (one-cancels-other when triggered).
- `OUO` are linked orders with `linked_order_ids` which are contingent on the other(s) (one-updates-other when triggered or modified).

```{note}
These contingency types relate to ContingencyType FIX tag <1385> https://www.onixs.biz/fix-dictionary/5.0.sp2/tagnum_1385.html.
```

### *'One Triggers the Other'* (OTO)
An OTO orders involves two orders—a parent order and a child order. The parent order is a live 
marketplace order. The child order, held in a separate order file, is not. If the parent order 
executes in full, the child order is released to the marketplace and becomes live. 
An OTO order can be made up of stock orders, option orders, or a combination of both.

### *'One Cancels the Other'* (OCO)
An OCO order is an order whose execution results in the immediate cancellation of another order 
linked to it. Cancellation of the Contingent Order happens on a best efforts basis. 
In an OCO order, both orders are live in the marketplace at the same time. The execution of either 
order triggers an attempt to cancel the other unexecuted order. Partial executions will also trigger an attempt to cancel the other order.

### *'One Updates the Other'* (OUO)
An OUO order is an order whose execution results in the immediate reduction of quantity in another 
order linked to it. The quantity reduction happens on a best effort basis. In an OUO order both 
orders are live in the marketplace at the same time. The execution of either order triggers an 
attempt to reduce the remaining quantity of the other order, partial executions included.


## Bracket Orders
Bracket orders are a type of advanced order that allows traders to set a take-profit and stop-loss 
level for a position at the same time. This is typically done by placing a parent order 
(also known as the entry order) and two child orders: a take-profit `LIMIT` order and a stop-loss `STOP_MARKET` order. 
When the parent order is executed, the two child orders are also placed in the market. 
If the market moves in favor of the trade, the take-profit order will be filled, closing the position and locking in profits. 
If the market moves against the trade, the stop-loss order will be filled, closing the position and limiting losses.

Bracket orders can be created using the [OrderFactory](https://docs.nautilustrader.io/api_reference/common.html#module-nautilus_trader.common.factories) class in the Nautilus Trader API. 
This allows for easy and efficient creation of bracket orders, with different order types, parameters and instructions.

```{warning}
You should be aware of the margin requirements of positions, as bracketing a position will consume 
more order margin.
```
