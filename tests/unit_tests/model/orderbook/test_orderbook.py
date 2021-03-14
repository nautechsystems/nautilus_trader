import pytest
from nautilus_trader.model.c_enums.order_side import OrderSide
from nautilus_trader.model.orderbook.ladder import Ladder
from nautilus_trader.model.orderbook.order import Order
from nautilus_trader.model.orderbook.orderbook import OrderbookProxy


@pytest.fixture(scope='function')
def empty_book():
    return OrderbookProxy()


def test_init():
    ob = OrderbookProxy()
    assert isinstance(ob.bids, Ladder) and isinstance(ob.asks, Ladder)
    assert ob.bids.reverse and not ob.asks.reverse


def test_add(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.BUY))
    assert empty_book.bids.top()[0].price() == 10.0


def test_top(empty_book):
    empty_book.add(Order(price=10, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=20, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=5, volume=5, side=OrderSide.BUY))
    empty_book.add(Order(price=25, volume=5, side=OrderSide.SELL))
    empty_book.add(Order(price=30, volume=5, side=OrderSide.SELL))
    empty_book.add(Order(price=21, volume=5, side=OrderSide.SELL))
    assert empty_book.top() == {}

# def test_auction_match_match_orders():
#     l1 = Ladder.from_orders(
#         [
#             Order(price=103, volume=5, side=BID),
#             Order(price=102, volume=10, side=BID),
#             Order(price=100, volume=5, side=BID),
#             Order(price=90, volume=5, side=BID),
#         ]
#     )
#     l2 = Ladder.from_orders(
#         [
#             Order(price=100, volume=10, side=ASK),
#             Order(price=101, volume=10, side=ASK),
#             Order(price=105, volume=5, side=ASK),
#             Order(price=110, volume=5, side=ASK),
#         ]
#     )
#     trades = l1.auction_match(l2, on="volume")
#     assert trades
#
#
# def test_insert_remaining():
#     bids = Ladder.from_orders(orders=[Order(price=103, volume=1, side=BID), Order(price=102, volume=1, side=BID)])
#     orderbook = Orderbook(bids=bids)
#
#     order = Order(price=100, volume=3, side=ASK)
#     trades = orderbook.insert(order=order)
#     assert trades[0].price == 103
#     assert trades[0].volume == 1
#     assert trades[1].price == 102
#     assert trades[1].volume == 1
#
#     assert orderbook.asks.top_level.price == 100
#     assert orderbook.asks.top_level.volume == 1
#
#
# def test_insert_book():
#     book = Orderbook()
#     for n in range(5):
#         book.insert(order=Order(price=n, volume=10, side=BID))
#
#
# def test_insert_in_cross_order(orderbook):
#     order = Order(price=100, volume=1, side=BID)
#     trades = orderbook.insert(order=order, remove_trades=True)
#     expected = [Order(price=1.2, volume=1.0, side=ASK, order_id="a4")]
#     assert trades == expected
#
#
# def test_exchange_order_ids():
#     book = Orderbook(bids=None, asks=None, exchange_order_ids=True)
#     assert book.exchange_order_ids
#     assert book.bids.exchange_order_ids
#     assert book.asks.exchange_order_ids
#
#
# def test_order_id_side(orderbook):
#     result = orderbook.loads(orderbook.dumps()).order_id_side
#     expected = orderbook.order_id_side
#     assert len(result) == 10
#     assert result == expected
#
#
# def test_orderbook_in_cross():
#     orderbook = Orderbook(bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]), asks=None)
#     assert not orderbook.in_cross
#     orderbook = Orderbook(
#         bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]),
#         asks=Ladder.from_orders(orders=[Order(price=10, volume=1, side=ASK)]),
#     )
#     assert orderbook.in_cross
