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
"""
Integration tests for futures expiry in the backtest engine.

Covers position closure at expiry with market price and custom_settlement_prices.

"""

import pandas as pd

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.core.datetime import dt_to_unix_nanos
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.data import TradeTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AggressorSide
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import FuturesContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


def test_future_expiry_with_custom_settlement():
    """
    Test futures position closed at expiry using custom settlement price.

    With custom_settlement_prices, the position is closed at the specified price instead
    of the market (last) price at expiry.

    """
    venue = Venue("XCME")
    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-06-21 14:30:00", tz="UTC"))
    activation_ns = dt_to_unix_nanos(pd.Timestamp("2024-01-01", tz="UTC"))

    future = FuturesContract(
        instrument_id=InstrumentId.from_str("ESM4.XCME"),
        raw_symbol=Symbol("ESM4"),
        asset_class=AssetClass.INDEX,
        currency=USD,
        price_precision=2,
        price_increment=Price.from_str("0.25"),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        underlying="ES",
        activation_ns=activation_ns,
        expiration_ns=expiry_ns,
        ts_event=activation_ns,
        ts_init=activation_ns,
    )

    custom_settlement = 6000.0

    engine = BacktestEngine(config=BacktestEngineConfig())

    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
        settlement_prices={future.id: custom_settlement},
    )

    engine.add_instrument(future)

    from nautilus_trader.model.enums import OrderSide

    class SimpleFuturesStrategy(Strategy):
        def __init__(self, config=None):
            super().__init__(config=config)
            self.order_submitted = False

        def on_start(self):
            self.subscribe_quote_ticks(future.id)

        def on_quote_tick(self, tick: QuoteTick):
            if tick.instrument_id == future.id and not self.order_submitted:
                order = self.order_factory.market(
                    instrument_id=future.id,
                    order_side=OrderSide.BUY,
                    quantity=Quantity.from_int(1),
                )
                self.submit_order(order)
                self.order_submitted = True

    engine.add_strategy(SimpleFuturesStrategy())

    # Quote before expiry to trigger order, trade at expiry to advance clock
    data = [
        QuoteTick(
            instrument_id=future.id,
            bid_price=Price(5995.0, 2),
            ask_price=Price(5995.25, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1_000_000_000,
            ts_init=expiry_ns - 1_000_000_000,
        ),
        TradeTick(
            instrument_id=future.id,
            price=Price(5990.0, 2),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)

    engine.run(start=pd.Timestamp(expiry_ns - 2_000_000_000, unit="ns", tz="UTC"))

    # Position should be closed at custom settlement (6000.0)
    positions = engine.cache.positions_open(venue=venue, instrument_id=future.id)
    assert len(positions) == 0, "Future position should be closed at expiration"

    # Find the expiration close order (tags contain EXPIRATION)
    for order in engine.cache.orders():
        if order.tags and "EXPIRATION" in " ".join(order.tags):
            assert float(order.avg_px) == custom_settlement, (
                f"Position should close at custom settlement {custom_settlement}, was {order.avg_px}"
            )
            break
    else:
        raise AssertionError("No expiration close order found")

    engine.dispose()


def test_future_expiry_at_market():
    """
    Test futures position closed at expiry using market price.

    Without custom_settlement_prices, the position closes via fill_market_order at
    expiry. Verifies the position is closed (exact fill price depends on book state).

    """
    venue = Venue("XCME")
    expiry_ns = dt_to_unix_nanos(pd.Timestamp("2024-06-21 14:30:00", tz="UTC"))
    activation_ns = dt_to_unix_nanos(pd.Timestamp("2024-01-01", tz="UTC"))

    future = FuturesContract(
        instrument_id=InstrumentId.from_str("ESM4.XCME"),
        raw_symbol=Symbol("ESM4"),
        asset_class=AssetClass.INDEX,
        currency=USD,
        price_precision=2,
        price_increment=Price.from_str("0.25"),
        multiplier=Quantity.from_int(1),
        lot_size=Quantity.from_int(1),
        underlying="ES",
        activation_ns=activation_ns,
        expiration_ns=expiry_ns,
        ts_event=activation_ns,
        ts_init=activation_ns,
    )

    engine = BacktestEngine(config=BacktestEngineConfig())
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=USD,
        starting_balances=[Money(1_000_000, USD)],
    )
    engine.add_instrument(future)

    from nautilus_trader.model.enums import OrderSide

    class SimpleFuturesStrategy(Strategy):
        def __init__(self, config=None):
            super().__init__(config=config)
            self.order_submitted = False

        def on_start(self):
            self.subscribe_quote_ticks(future.id)

        def on_quote_tick(self, tick: QuoteTick):
            if tick.instrument_id == future.id and not self.order_submitted:
                order = self.order_factory.market(
                    instrument_id=future.id,
                    order_side=OrderSide.BUY,
                    quantity=Quantity.from_int(1),
                )
                self.submit_order(order)
                self.order_submitted = True

    engine.add_strategy(SimpleFuturesStrategy())

    data = [
        QuoteTick(
            instrument_id=future.id,
            bid_price=Price(5995.0, 2),
            ask_price=Price(5995.25, 2),
            bid_size=Quantity.from_int(10),
            ask_size=Quantity.from_int(10),
            ts_event=expiry_ns - 1_000_000_000,
            ts_init=expiry_ns - 1_000_000_000,
        ),
        TradeTick(
            instrument_id=future.id,
            price=Price(5992.5, 2),
            size=Quantity.from_int(1),
            aggressor_side=AggressorSide.NO_AGGRESSOR,
            trade_id=TradeId("T1"),
            ts_event=expiry_ns,
            ts_init=expiry_ns,
        ),
    ]
    engine.add_data(data)
    engine.run(start=pd.Timestamp(expiry_ns - 2_000_000_000, unit="ns", tz="UTC"))

    positions = engine.cache.positions_open(venue=venue, instrument_id=future.id)
    assert len(positions) == 0, "Future position should be closed at expiration"

    exp_orders = [o for o in engine.cache.orders() if o.tags and "EXPIRATION" in " ".join(o.tags)]
    assert len(exp_orders) >= 1, "Should have expiration close order"

    engine.dispose()
