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

from nautilus_trader.backtest.engine import BacktestEngine
from nautilus_trader.backtest.engine import BacktestEngineConfig
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.data import QuoteTick
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import AssetClass
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OptionKind
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments import OptionContract
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.trading.strategy import Strategy


class OptionAccountingTestStrategy(Strategy):
    def __init__(self, instrument_id: InstrumentId, order_side: OrderSide, config=None):
        super().__init__(config=config)
        self.instrument_id = instrument_id
        self.order_side = order_side
        self.order_submitted = False

    def on_start(self):
        self.subscribe_quote_ticks(self.instrument_id)

    def on_quote_tick(self, tick):
        if tick.instrument_id == self.instrument_id and not self.order_submitted:
            order = self.order_factory.market(
                instrument_id=self.instrument_id,
                order_side=self.order_side,
                quantity=Quantity.from_int(1),
            )
            self.submit_order(order)
            self.order_submitted = True


@pytest.mark.parametrize("account_type", [AccountType.CASH, AccountType.MARGIN])
@pytest.mark.parametrize("order_side", [OrderSide.BUY, OrderSide.SELL])
@pytest.mark.parametrize("multiplier", [1, 10, 100])
def test_option_cash_balance_impact(account_type, order_side, multiplier):
    """
    Test that buying or selling an option correctly impacts the cash balance, accounting
    for the instrument's multiplier and upfront premium payment.
    """
    venue = Venue("NASDAQ")
    engine = BacktestEngine(config=BacktestEngineConfig())

    starting_balance = Money(1_000_000, USD)
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=account_type,
        base_currency=USD,
        starting_balances=[starting_balance],
    )

    # Option: price $5.00
    price = 5.0
    quantity = 1
    expected_impact = Decimal(str(price)) * Decimal(str(quantity)) * Decimal(str(multiplier))

    option = OptionContract(
        instrument_id=InstrumentId.from_str(f"AAPL-{multiplier}.NASDAQ"),
        raw_symbol=Symbol(f"AAPL-{multiplier}"),
        asset_class=AssetClass.EQUITY,
        underlying="AAPL",
        option_kind=OptionKind.CALL,
        strike_price=Price(150.0, 2),
        currency=USD,
        activation_ns=0,
        expiration_ns=int(1e18),
        price_precision=2,
        price_increment=Price(0.01, 2),
        multiplier=Quantity.from_int(multiplier),
        lot_size=Quantity.from_int(1),
        ts_event=0,
        ts_init=0,
    )
    engine.add_instrument(option)

    strategy = OptionAccountingTestStrategy(option.id, order_side)
    engine.add_strategy(strategy)

    # Quote for option to trigger strategy order
    ts = 1_000_000
    engine.add_data(
        [
            QuoteTick(
                instrument_id=option.id,
                bid_price=Price(price, 2),
                ask_price=Price(price, 2),
                bid_size=Quantity.from_int(10),
                ask_size=Quantity.from_int(10),
                ts_event=ts,
                ts_init=ts,
            ),
        ],
    )

    engine.run()

    account = engine.cache.accounts()[0]
    balance = account.balance(USD).total

    expected_balance = starting_balance.as_decimal()
    if order_side == OrderSide.BUY:
        expected_balance -= expected_impact
    else:
        expected_balance += expected_impact

    assert balance.as_decimal() == expected_balance

    engine.dispose()
