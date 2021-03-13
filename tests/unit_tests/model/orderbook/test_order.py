from nautilus_trader.model.c_enums.order_side import OrderSide

from nautilus_trader.model.orderbook.order import Order


def test_init():
    order = Order(price=100.0, volume=10.0, side=OrderSide.BUY)
    assert order.price == 100
    assert order.volume == 10
    assert order.side == OrderSide.BUY


def test_order_id():
    order = Order(price=100.0, volume=10.0, side=OrderSide.BUY, id='1')
    assert order.id == '1'

    order = Order(price=100.0, volume=10.0, side=OrderSide.BUY)
    assert len(order.id) == 36


def test_update():
    order = Order(price=100.0, volume=10.0, side=OrderSide.BUY)
    order.update_price(price=90)
    assert order.price == 90.0
    order.update_volume(volume=5)
    assert order.volume == 5.0


# def test_exposure():
#     order = Order(price=100.0, volume=10.0, side=OrderSide.BUY)
#     assert order.exposure == 1000


# def test_signed_volume():
#     order = Order(price=10, volume=1, side=BID)
#     assert order.volume == 1 and order.signed_volume == 1
#
#     order = Order(price=10, volume=5, side=ASK)
#     assert order.volume == 5 and order.signed_volume == -5.0
#
#     order = Order(price=10, volume=0, side=ASK)
#     assert order.volume == 0 and order.signed_volume == 0
