from cumulo.protocol.models.order import Order
from cumulo.protocol.enums import BID, ASK


def test_init():
    bid_order = Order(price=100, volume=10, side=BID)
    ask_order = Order(price=100, volume=10, side=ASK)

    assert bid_order.price == 100
    assert bid_order.volume == 10
    assert bid_order.side == BID
    assert type(bid_order.__str__()) == str
    assert ask_order.exposure == 1000


def test_order_transform():
    def transform(o):
        return o.replace(price=o.price * 2, volume=o.volume / 2)

    order = Order(price=10, volume=10, side=BID)

    new_order = transform(order)
    assert new_order.price == 20.0 and new_order.volume == 5.0


def test_dict():
    order = Order(price=10, volume=0, side=BID)
    expected = {"price": 10.0, "side": BID, "volume": 0.0, "order_id": order.order_id}
    result = order.dict()
    assert result == expected


def test_serialization():
    order = Order(price=10, volume=0, side=BID)
    raw = order.dumps()
    assert len(raw) == 165
    result = order.loads(raw)
    assert result == order


def test_signed_volume():
    order = Order(price=10, volume=1, side=BID)
    assert order.volume == 1 and order.signed_volume == 1

    order = Order(price=10, volume=5, side=ASK)
    assert order.volume == 5 and order.signed_volume == -5.0

    order = Order(price=10, volume=0, side=ASK)
    assert order.volume == 0 and order.signed_volume == 0
