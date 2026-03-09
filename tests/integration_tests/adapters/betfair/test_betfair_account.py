from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_price
from nautilus_trader.adapters.betfair.orderbook import betfair_float_to_quantity


def test_betting_instrument_notional_value(instrument):
    notional = instrument.notional_value(
        price=betfair_float_to_price(2.0),
        quantity=betfair_float_to_quantity(100.0),
    ).as_double()
    assert notional == 100
