from cumulo.protocol.enums import BID, ASK
from cumulo.protocol.models.ladder import Ladder
from cumulo.protocol.models.order import Order
from cumulo.protocol.models.orderbook import Orderbook, Level


def _compare_ladder(ladder1: Ladder, ladder2: Ladder):
    return all([(l1.price, l1.volume) == (l2.price, l2.volume) for l1, l2 in zip(ladder1.levels, ladder2.levels)])


def _compare_orderbook(ob1, ob2):
    return _compare_ladder(ob1.bids, ob2.bids) and _compare_ladder(ob1.asks, ob2.asks)


def test_init():
    bid_list = [Order(price=110, volume=10, side=BID), Order(price=100, volume=5, side=BID)]
    ask_list = [Order(price=120, volume=1, side=ASK), Order(price=130, volume=5, side=ASK)]
    bid_ladder = Ladder.from_orders(orders=bid_list)
    ask_ladder = Ladder.from_orders(orders=ask_list)

    # Objects | 16.7 µs ± 70.9 ns per loop |
    ob = Orderbook(
        bids=Ladder(
            levels=[
                Level(orders=[Order(price=100, volume=5, side=BID)]),
                Level(orders=[Order(price=110, volume=10, side=BID)]),
            ]
        ),
        asks=Ladder(
            levels=[
                Level(orders=[Order(price=120, volume=1, side=ASK)]),
                Level(orders=[Order(price=130, volume=5, side=ASK)]),
            ]
        ),
    )
    assert _compare_ladder(ob.bids, bid_ladder) and _compare_ladder(ob.asks, ask_ladder)


def test_orderbook_transform():
    ob = Orderbook(
        bids=Ladder(levels=[Level(orders=[Order(price=100, volume=5, side=BID)])], side=BID),
        asks=Ladder(levels=[Level(orders=[Order(price=120, volume=1, side=ASK)])], side=ASK),
    )

    def double_volume(order):
        return order.replace(volume=order.volume * 2)

    assert ob.bids.depth_at_price(100) == 5
    assert ob.asks.depth_at_price(120) == 1

    ob.transform(double_volume)

    assert ob.bids.depth_at_price(100) == 10
    assert ob.asks.depth_at_price(120) == 2


def test_auction_match_match_orders():
    l1 = Ladder.from_orders(
        [
            Order(price=103, volume=5, side=BID),
            Order(price=102, volume=10, side=BID),
            Order(price=100, volume=5, side=BID),
            Order(price=90, volume=5, side=BID),
        ]
    )
    l2 = Ladder.from_orders(
        [
            Order(price=100, volume=10, side=ASK),
            Order(price=101, volume=10, side=ASK),
            Order(price=105, volume=5, side=ASK),
            Order(price=110, volume=5, side=ASK),
        ]
    )
    trades = l1.auction_match(l2, on="volume")
    assert trades


def test_insert_remaining():
    bids = Ladder.from_orders(orders=[Order(price=103, volume=1, side=BID), Order(price=102, volume=1, side=BID)])
    orderbook = Orderbook(bids=bids)

    order = Order(price=100, volume=3, side=ASK)
    trades = orderbook.insert(order=order)
    assert trades[0].price == 103
    assert trades[0].volume == 1
    assert trades[1].price == 102
    assert trades[1].volume == 1

    assert orderbook.asks.top_level.price == 100
    assert orderbook.asks.top_level.volume == 1


def test_insert_book():
    book = Orderbook()
    for n in range(5):
        book.insert(order=Order(price=n, volume=10, side=BID))


def test_insert_in_cross_order(orderbook):
    order = Order(price=100, volume=1, side=BID)
    trades = orderbook.insert(order=order, remove_trades=True)
    expected = [Order(price=1.2, volume=1.0, side=ASK, order_id="a4")]
    assert trades == expected


def test_serialization():
    bids = Ladder.from_orders(
        [
            Order(price=103, volume=5, side=BID),
            Order(price=102, volume=10, side=BID),
            Order(price=100, volume=5, side=BID),
            Order(price=90, volume=5, side=BID),
        ]
    )
    asks = Ladder.from_orders(
        [
            Order(price=100, volume=10, side=ASK),
            Order(price=101, volume=10, side=ASK),
            Order(price=105, volume=5, side=ASK),
            Order(price=110, volume=5, side=ASK),
        ]
    )

    book = Orderbook(bids=bids, asks=asks)
    raw = book.dumps()
    assert len(raw) == 2578
    result = book.loads(raw)
    assert result == book


def test_exchange_order_ids():
    book = Orderbook(bids=None, asks=None, exchange_order_ids=True)
    assert book.exchange_order_ids
    assert book.bids.exchange_order_ids
    assert book.asks.exchange_order_ids


def test_order_id_side(orderbook):
    result = orderbook.loads(orderbook.dumps()).order_id_side
    expected = orderbook.order_id_side
    assert len(result) == 10
    assert result == expected


def test_orderbook_flatten(orderbook):
    data = orderbook.flatten()
    expected = {
        "orderbook_bid_price_1": 11.0,
        "orderbook_bid_volume_1": 7.11,
        "orderbook_ask_price_1": 1.2,
        "orderbook_ask_volume_1": 2.85,
    }
    assert data == expected


def test_orderbook_empty_flatten():
    orderbook = Orderbook(bids=None, asks=None)
    data = orderbook.flatten()
    expected = {}
    assert data == expected


def test_orderbook_in_cross():
    orderbook = Orderbook(bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]), asks=None)
    assert not orderbook.in_cross
    orderbook = Orderbook(
        bids=Ladder.from_orders(orders=[Order(price=15, volume=1, side=BID)]),
        asks=Ladder.from_orders(orders=[Order(price=10, volume=1, side=ASK)]),
    )
    assert orderbook.in_cross
