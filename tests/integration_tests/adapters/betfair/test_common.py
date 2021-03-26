from adapters.betfair.common import price_to_probability
from adapters.betfair.common import probability_to_price
from adapters.betfair.common import round_price
from adapters.betfair.common import round_probability
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.objects import Price


def test_round_probability():
    assert round_probability(0.5, side=OrderSide.BUY) == 0.5
    assert round_probability(0.49999, side=OrderSide.BUY) == 0.49505
    assert round_probability(0.49999, side=OrderSide.SELL) == 0.5


def test_round_price():
    # Test rounding betting prices
    assert round_price(2.0, side=OrderSide.BUY) == 2.0
    assert round_price(2.01, side=OrderSide.BUY) == 2.02
    assert round_price(2.01, side=OrderSide.SELL) == 2.0


def test_price_to_probability():
    # Exact match
    assert price_to_probability(1.69, side=OrderSide.BUY) == Price("0.59172")
    # Rounding match
    assert price_to_probability(2.01, side=OrderSide.BUY) == Price("0.49505")
    assert price_to_probability(2.01, side=OrderSide.SELL) == Price("0.50000")


def test_probability_to_price():
    # Exact match
    assert probability_to_price(0.5, side=OrderSide.BUY) == Price("2.0")
    # Rounding match
    assert probability_to_price(0.499, side=OrderSide.BUY) == Price("2.02")
    assert probability_to_price(0.501, side=OrderSide.BUY) == Price("2.0")
    assert probability_to_price(0.501, side=OrderSide.SELL) == Price("1.99")
