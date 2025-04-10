# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.accounting.accounts.cash import CashAccount
from nautilus_trader.accounting.manager import AccountsManager
from nautilus_trader.common.component import Logger
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import ADA
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import JPY
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.events import OrderFilled
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.execution import TestExecStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
ADABTC_BINANCE = TestInstrumentProvider.adabtc_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()
AAPL_XNAS = TestInstrumentProvider.equity(symbol="AAPL", venue="XNAS")


class TestCashAccount:
    def setup(self):
        # Fixture Setup
        self.trader_id = TestIdStubs.trader_id()

        self.order_factory = OrderFactory(
            trader_id=self.trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

    def test_instantiated_accounts_basic_properties(self):
        # Arrange, Act
        account = TestExecStubs.cash_account()

        # Assert
        assert account == account
        assert account == account
        assert account.id == AccountId("SIM-000")
        assert str(account) == "CashAccount(id=SIM-000, type=CASH, base=USD)"
        assert repr(account) == "CashAccount(id=SIM-000, type=CASH, base=USD)"
        assert isinstance(hash(account), int)

    def test_is_unleveraged_returns_true(self):
        # Arrange, Act
        account = TestExecStubs.cash_account()

        # Assert
        assert account.is_unleveraged(AUDUSD_SIM.id)

    def test_instantiate_single_asset_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = CashAccount(event)

        # Assert
        assert account.base_currency == USD
        assert account.last_event == event
        assert account.events == [event]
        assert account.event_count == 1
        assert account.balance_total() == Money(1_000_000, USD)
        assert account.balance_free() == Money(1_000_000, USD)
        assert account.balance_locked() == Money(0, USD)
        assert account.balances_total() == {USD: Money(1_000_000, USD)}
        assert account.balances_free() == {USD: Money(1_000_000, USD)}
        assert account.balances_locked() == {USD: Money(0, USD)}

    def test_instantiate_multi_asset_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = CashAccount(event)

        # Assert
        assert account.id == AccountId("SIM-000")
        assert account.base_currency is None
        assert account.last_event == event
        assert account.events == [event]
        assert account.event_count == 1
        assert account.balance_total(BTC) == Money(10.00000000, BTC)
        assert account.balance_total(ETH) == Money(20.00000000, ETH)
        assert account.balance_free(BTC) == Money(10.00000000, BTC)
        assert account.balance_free(ETH) == Money(20.00000000, ETH)
        assert account.balance_locked(BTC) == Money(0.00000000, BTC)
        assert account.balance_locked(ETH) == Money(0.00000000, ETH)
        assert account.balances_total() == {
            BTC: Money(10.00000000, BTC),
            ETH: Money(20.00000000, ETH),
        }
        assert account.balances_free() == {
            BTC: Money(10.00000000, BTC),
            ETH: Money(20.00000000, ETH),
        }
        assert account.balances_locked() == {
            BTC: Money(0.00000000, BTC),
            ETH: Money(0.00000000, ETH),
        }

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event1 = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = CashAccount(event1)

        event2 = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(9.00000000, BTC),
                    Money(0.50000000, BTC),
                    Money(8.50000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account.apply(event=event2)

        # Assert
        assert account.last_event == event2
        assert account.events == [event1, event2]
        assert account.event_count == 2
        assert account.balance_total(BTC) == Money(9.00000000, BTC)
        assert account.balance_free(BTC) == Money(8.50000000, BTC)
        assert account.balance_locked(BTC) == Money(0.50000000, BTC)
        assert account.balance_total(ETH) == Money(20.00000000, ETH)
        assert account.balance_free(ETH) == Money(20.00000000, ETH)
        assert account.balance_locked(ETH) == Money(0.00000000, ETH)

    def test_calculate_balance_locked_buy(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=AUDUSD_SIM,
            side=OrderSide.BUY,
            quantity=Quantity.from_int(1_000_000),
            price=Price.from_str("0.80"),
        )

        # Assert
        assert result == Money(800_032.00, USD)  # Notional + expected commission

    def test_calculate_balance_locked_sell(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=AUDUSD_SIM,
            side=OrderSide.SELL,
            quantity=Quantity.from_int(1_000_000),
            price=Price.from_str("0.80"),
        )

        # Assert
        assert result == Money(1_000_040.00, AUD)  # Notional + expected commission

    def test_calculate_balance_locked_sell_no_base_currency(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        # Act
        result = account.calculate_balance_locked(
            instrument=AAPL_XNAS,
            side=OrderSide.SELL,
            quantity=Quantity.from_int(100),
            price=Price.from_str("1500.00"),
        )

        # Assert
        assert result == Money(100.00, USD)  # Notional + expected commission

    def test_calculate_pnls_for_single_currency_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("0.80000"),
        )

        position = Position(AUDUSD_SIM, fill)

        # Act
        result = account.calculate_pnls(
            instrument=AUDUSD_SIM,
            fill=fill,
            position=position,
        )

        # Assert (does not include commission)
        assert result == [Money(-800000.00, USD)]

    def test_calculate_pnls_for_multi_currency_cash_account_btcusdt(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        order1 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.500000"),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("45500.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill1)

        # Act
        result1 = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill1,
            position=position,
        )

        order2 = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_str("0.500000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("45500.00"),
        )

        result2 = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            fill=fill2,
            position=position,
        )

        # Assert (does not include commission)
        assert result1 == [Money(-0.50000000, BTC), Money(22750.00000000, USDT)]
        assert result2 == [Money(0.50000000, BTC), Money(-22750.00000000, USDT)]

    def test_calculate_pnls_for_multi_currency_cash_account_adabtc(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    Money(1.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(1.00000000, BTC),
                ),
                AccountBalance(
                    Money(1000.00000000, ADA),
                    Money(0.00000000, ADA),
                    Money(1000.00000000, ADA),
                ),
            ],
            margins=[],
            info={},  # No default currency set
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        account = CashAccount(event)

        order = self.order_factory.market(
            ADABTC_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        fill = TestEventStubs.order_filled(
            order,
            instrument=ADABTC_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("0.00004100"),
        )

        position = Position(ADABTC_BINANCE, fill)

        # Act
        result = account.calculate_pnls(
            instrument=ADABTC_BINANCE,
            fill=fill,
            position=position,
        )

        # Assert (does not include commission)
        assert result == [Money(100.000000, ADA), Money(-0.00410000, BTC)]

    def test_calculate_commission_when_given_liquidity_side_none_raises_value_error(
        self,
    ):
        # Arrange
        account = TestExecStubs.cash_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act, Assert
        with pytest.raises(ValueError):
            account.calculate_commission(
                instrument=instrument,
                last_qty=Quantity.from_int(100_000),
                last_px=Price.from_str("11450.50"),
                liquidity_side=LiquiditySide.NO_LIQUIDITY_SIDE,
            )

    @pytest.mark.parametrize(
        ("use_quote_for_inverse", "expected"),
        [
            [False, Money(-0.00218331, BTC)],  # Negative commission = credit
            [True, Money(-25.00, USD)],  # Negative commission = credit
        ],
    )
    def test_calculate_commission_for_inverse_maker_crypto(self, use_quote_for_inverse, expected):
        # Arrange
        account = TestExecStubs.cash_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(100_000),
            last_px=Price.from_str("11450.50"),
            liquidity_side=LiquiditySide.MAKER,
            use_quote_for_inverse=use_quote_for_inverse,
        )

        # Assert
        assert result == expected

    def test_calculate_commission_for_taker_fx(self):
        # Arrange
        account = TestExecStubs.cash_account()
        instrument = AUDUSD_SIM

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(1_500_000),
            last_px=Price.from_str("0.80050"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(24.02, USD)

    def test_calculate_commission_crypto_taker(self):
        # Arrange
        account = TestExecStubs.cash_account()
        instrument = TestInstrumentProvider.xbtusd_bitmex()

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(100_000),
            last_px=Price.from_str("11450.50"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(0.00654993, BTC)

    def test_calculate_commission_fx_taker(self):
        # Arrange
        account = TestExecStubs.cash_account()
        instrument = TestInstrumentProvider.default_fx_ccy("USD/JPY", Venue("IDEALPRO"))

        # Act
        result = account.calculate_commission(
            instrument=instrument,
            last_qty=Quantity.from_int(2_200_000),
            last_px=Price.from_str("120.310"),
            liquidity_side=LiquiditySide.TAKER,
        )

        # Assert
        assert result == Money(5294, JPY)


def test_cash_account_eth_usdt_balance_calculation():
    # Arrange
    event = AccountState(
        account_id=AccountId("BINANCE-001"),
        account_type=AccountType.CASH,
        base_currency=None,  # Multi-currency account
        reported=False,
        balances=[
            AccountBalance(
                Money(3_655.22600905, USDT),
                Money(0.00000000, USDT),
                Money(3_655.22600905, USDT),
            ),
            AccountBalance(
                Money(3_946.76679000, ETH),
                Money(0.00000000, ETH),
                Money(3_946.76679000, ETH),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(event)

    # Act - Simulate locking the entire ETH balance
    eth_usdt_id = InstrumentId.from_str("ETHUSDT.BINANCE")
    locked_eth = Money(3_946.76679000, ETH)
    account.update_balance_locked(eth_usdt_id, locked_eth)

    # Assert
    eth_balance = account.balance(ETH)
    usdt_balance = account.balance(USDT)

    assert eth_balance.total == Money(3_946.76679000, ETH)
    assert eth_balance.locked == Money(3_946.76679000, ETH)
    assert eth_balance.free == Money(0.0, ETH)

    assert usdt_balance.total == Money(3_655.22600905, USDT)
    assert usdt_balance.locked == Money(0.0, USDT)
    assert usdt_balance.free == Money(3_655.22600905, USDT)


def test_cash_account_update_with_fill_to_zero():
    # Arrange
    clock = TestClock()
    cache = TestComponentStubs.cache()
    logger = Logger("Portfolio")

    instrument = TestInstrumentProvider.ethusdt_binance()
    cache.add_instrument(instrument)

    event = AccountState(
        account_id=AccountId("BINANCE-001"),
        account_type=AccountType.CASH,
        base_currency=None,  # Multi-currency account
        reported=False,
        balances=[
            AccountBalance(
                Money(10000.0, USDT),
                Money(0.0, USDT),
                Money(10000.0, USDT),
            ),
            AccountBalance(
                Money(10.0, ETH),
                Money(0.0, ETH),
                Money(10.0, ETH),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )

    # Create order fill event that sells all ETH
    fill = OrderFilled(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId("O-20221110-001"),
        venue_order_id=VenueOrderId("V-001"),
        account_id=account.id,
        trade_id=TradeId("T-001"),
        order_side=OrderSide.SELL,
        order_type=OrderType.LIMIT,
        last_qty=Quantity.from_str("10.00000"),  # Entire ETH balance
        last_px=Price.from_str("10_000.00000"),
        currency=USDT,
        commission=Money(10.0, USDT),
        liquidity_side=LiquiditySide.TAKER,
        position_id=PositionId("ETH"),
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    # Act
    accounts_manager.update_balances(
        account=account,
        instrument=instrument,
        fill=fill,
    )

    # Assert
    eth_balance = account.balance(ETH)
    usdt_balance = account.balance(USDT)

    # ETH balance should be zero
    assert eth_balance.total.as_decimal() == Decimal("0.00000000")
    assert eth_balance.locked.as_decimal() == Decimal("0.00000000")
    assert eth_balance.free.as_decimal() == Decimal("0.00000000")

    # USDT balance should be increased by the trade value minus commission
    assert usdt_balance.total.as_decimal() == Decimal("109990.00000000")
    assert usdt_balance.locked.as_decimal() == Decimal("0.00000000")
    assert usdt_balance.free.as_decimal() == Decimal("109990.00000000")
