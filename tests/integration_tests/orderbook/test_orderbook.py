import gzip
import json

import pytest
from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.orderbook.orderbook import L3Orderbook


@pytest.fixture()
def l2_feed():
    return [json.loads(line) for line in gzip.open("./resources/L2_feed.log.gz")]


@pytest.fixture()
def l3_feed():
    def parser(line):
        parsed = json.loads(line.decode())
        if not isinstance(parsed, list):
            # print(parsed)
            return
        elif isinstance(parsed, list):
            channel, updates = parsed
            if not isinstance(updates[0], list):
                updates = [updates]
        else:
            raise KeyError()
        if isinstance(updates, int):
            print("Err", updates)
            return
        for values in updates:
            keys = ("order_id", "price", "volume")
            data = dict(zip(keys, values))
            side = OrderSide.BUY if data["volume"] >= 0 else OrderSide.SELL
            if data["price"] == 0:
                yield dict(
                    op="delete",
                    order=Order(price=data["price"], volume=abs(data["volume"]), side=side, id=str(data["order_id"]))
                )
            else:
                yield dict(
                    op="update",
                    order=Order(price=data["price"], volume=abs(data["volume"]), side=side, id=str(data["order_id"]))
                )

    return [msg for line in gzip.open("resources/bitfinex_L3_feed.log.gz") for msg in parser(line)]


def test_l3_feed(l3_feed):
    ob = L3Orderbook()
    # Updates that cause the book to fail integrity checks will be deleted immediately, but we may get also delete later
    skip_deletes = []

    for i, m in enumerate(l3_feed):
        if m['op'] == 'update':
            ob.update(order=m['order'])
            if not ob._check_integrity(deep=False):
                ob.delete(order=m['order'])
                skip_deletes.append(m['order'].id)
        elif m['op'] == 'delete' and m['order'].id not in skip_deletes:
            ob.delete(order=m['order'])
        assert ob._check_integrity(deep=False)
    assert i == 100_047
    assert ob.best_ask.price == 61405.27923706 and ob.best_ask.volume == 0.12227
    assert ob.best_bid.price == 61391 and ob.best_bid.volume == 1

# def test_l2_feed(l2_feed):
#     ob = Orderbook()
#     for m in l2_feed[:10]:
#         if m['type'] == 'book_update':
#             pass
