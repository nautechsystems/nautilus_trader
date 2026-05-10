# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
#
#  Licensed under the GNU Lesser General Public License Version 3.0 (the "License");
#  You may not use this file except in compliance with the License.
#  You may obtain a copy of the License at https://www.gnu.org/licenses/lgpl-3.0.en.html
#
#  Unless required by applicable law or agreed to in writing, software
#  distributed under the License is distributed on an "AS IS" BASIS,
#  WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
#  See the License for the specific language governing permissions and
#  limitations under the License.
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

import pytest

from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderStatus
from nautilus_trader.model import OwnOrderBook
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from tests.unit.model.factories import make_own_order


@pytest.fixture
def book(audusd_id):
    return OwnOrderBook(instrument_id=audusd_id)


def test_own_order_book_construction(book, audusd_id):
    assert book.instrument_id == audusd_id
    assert book.update_count == 0


def test_own_book_order_construction():
    order = make_own_order()

    assert order.client_order_id == ClientOrderId("O-001")
    assert order.side == OrderSide.BUY
    assert order.price == Price.from_str("1.00000")
    assert order.size == Quantity.from_int(100_000)


def test_own_book_order_hash():
    order = make_own_order()

    assert isinstance(hash(order), int)


def test_add_and_query(book):
    bid = make_own_order(side=OrderSide.BUY, price="1.00000", client_order_id="O-001")
    ask = make_own_order(side=OrderSide.SELL, price="1.00010", client_order_id="O-002")

    book.add(bid)
    book.add(ask)

    assert book.is_order_in_book(ClientOrderId("O-001"))
    assert book.is_order_in_book(ClientOrderId("O-002"))
    assert not book.is_order_in_book(ClientOrderId("O-999"))
    assert len(book.orders_to_list()) == 2
    assert len(book.bids_to_list()) == 1
    assert len(book.asks_to_list()) == 1


def test_bid_and_ask_client_order_ids(book):
    bid = make_own_order(side=OrderSide.BUY, client_order_id="O-001")
    ask = make_own_order(side=OrderSide.SELL, price="1.00010", client_order_id="O-002")

    book.add(bid)
    book.add(ask)

    assert book.bid_client_order_ids() == [ClientOrderId("O-001")]
    assert book.ask_client_order_ids() == [ClientOrderId("O-002")]


def test_delete(book):
    order = make_own_order()
    book.add(order)

    assert book.is_order_in_book(ClientOrderId("O-001"))

    book.delete(order)

    assert not book.is_order_in_book(ClientOrderId("O-001"))
    assert len(book.orders_to_list()) == 0


def test_clear(book):
    book.add(make_own_order(client_order_id="O-001"))
    book.add(make_own_order(side=OrderSide.SELL, price="1.00010", client_order_id="O-002"))

    book.clear()

    assert len(book.orders_to_list()) == 0


def test_reset(book):
    book.add(make_own_order())

    book.reset()

    assert len(book.orders_to_list()) == 0


def test_update(book):
    book.add(make_own_order(client_order_id="O-001"))

    book.update(
        make_own_order(
            price="1.00010",
            size=120_000,
            client_order_id="O-001",
            status=OrderStatus.PARTIALLY_FILLED,
        ),
    )

    updated = book.orders_to_list()[0]

    assert updated.price == Price.from_str("1.00010")
    assert updated.size == Quantity.from_int(120_000)
    assert updated.status == OrderStatus.PARTIALLY_FILLED


def test_bids_and_asks_to_dict(book):
    book.add(make_own_order(client_order_id="O-001"))
    book.add(make_own_order(side=OrderSide.SELL, price="1.00010", client_order_id="O-002"))

    bids = book.bids_to_dict()
    asks = book.asks_to_dict()

    assert list(bids.keys()) == [Decimal("1.00000")]
    assert [order.client_order_id for order in bids[Decimal("1.00000")]] == [
        ClientOrderId("O-001"),
    ]
    assert list(asks.keys()) == [Decimal("1.00010")]
    assert [order.client_order_id for order in asks[Decimal("1.00010")]] == [
        ClientOrderId("O-002"),
    ]


def test_bids_to_dict_filters_status_and_accepted_buffer(book):
    book.add(
        make_own_order(
            client_order_id="O-001",
            ts_last=10,
            ts_accepted=10,
            status=OrderStatus.ACCEPTED,
        ),
    )
    book.add(
        make_own_order(
            price="0.99990",
            size=50_000,
            client_order_id="O-002",
            ts_last=20,
            ts_submitted=20,
            status=OrderStatus.SUBMITTED,
        ),
    )

    accepted_only = book.bids_to_dict(status={OrderStatus.ACCEPTED})
    buffered = book.bids_to_dict(
        status={OrderStatus.ACCEPTED},
        accepted_buffer_ns=15,
        ts_now=20,
    )

    assert list(accepted_only.keys()) == [Decimal("1.00000")]
    assert [order.client_order_id for order in accepted_only[Decimal("1.00000")]] == [
        ClientOrderId("O-001"),
    ]
    assert buffered == {}


def test_bid_and_ask_quantity_views(book):
    book.add(make_own_order(client_order_id="O-001"))
    book.add(make_own_order(price="0.99990", size=50_000, client_order_id="O-002"))
    book.add(
        make_own_order(
            side=OrderSide.SELL,
            price="1.00010",
            size=70_000,
            client_order_id="O-003",
        ),
    )

    assert book.bid_quantity() == {
        Decimal("1.00000"): Decimal(100000),
        Decimal("0.99990"): Decimal(50000),
    }
    assert book.ask_quantity() == {Decimal("1.00010"): Decimal(70000)}
    assert book.bid_quantity(depth=1) == {Decimal("1.00000"): Decimal(100000)}
    assert book.bid_quantity(group_size=Decimal("0.0001")) == {
        Decimal("1.0000"): Decimal(100000),
        Decimal("0.9999"): Decimal(50000),
    }


def test_combined_with_opposite_uses_primary_instrument(audusd_id, usdjpy_id):
    primary = OwnOrderBook(instrument_id=audusd_id)
    opposite = OwnOrderBook(instrument_id=usdjpy_id)

    primary.add(make_own_order(client_order_id="O-001"))
    opposite.add(
        make_own_order(
            side=OrderSide.SELL,
            price="150.000",
            size=50_000,
            client_order_id="O-002",
        ),
    )

    combined = primary.combined_with_opposite(opposite)

    assert combined.instrument_id == audusd_id
    assert {order.client_order_id for order in combined.orders_to_list()} == {
        ClientOrderId("O-001"),
        ClientOrderId("O-002"),
    }


def test_audit_open_orders_removes_missing_orders(book):
    book.add(make_own_order(client_order_id="O-001"))
    book.add(make_own_order(price="0.99990", client_order_id="O-002"))

    book.audit_open_orders({ClientOrderId("O-001")})

    assert [order.client_order_id for order in book.orders_to_list()] == [ClientOrderId("O-001")]

    book.audit_open_orders({ClientOrderId("O-999")})

    assert book.orders_to_list() == []


def test_pprint(book):
    book.add(make_own_order(client_order_id="O-001"))
    book.add(make_own_order(side=OrderSide.SELL, price="1.00010", client_order_id="O-002"))

    result = book.pprint()

    assert "bid_levels: 1" in result
    assert "ask_levels: 1" in result
    assert "1.00010" in result
