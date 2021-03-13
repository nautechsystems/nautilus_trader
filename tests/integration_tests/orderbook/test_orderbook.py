import json

import pytest
from nautilus_trader.model.orderbook.orderbook import Orderbook


@pytest.fixture()
def feed():
    return [json.loads(line) for line in open("./resources/L2_feed.log")]


def test_protocol(feed):
    for m in feed[:10]:
        print(m)
        # if m['type'] == 'book_update':
