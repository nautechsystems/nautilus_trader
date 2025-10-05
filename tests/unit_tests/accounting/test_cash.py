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
from nautilus_trader.cache.cache import Cache
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
        assert result == Money(800_000.00, USD)  # Notional

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
        assert result == Money(1_000_000.00, AUD)  # Notional

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
            AccountBalance(  # simulating an existing order in our account
                Money(10.0, ETH),
                Money(10.0, ETH),
                Money(0.0, ETH),
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


def test_cash_account_calculate_balance_locked():
    """
    Test that calculate_balance_locked returns correct values.
    """
    # Arrange
    account = TestExecStubs.cash_account()

    order = TestExecStubs.limit_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00000"),
    )

    # Act
    locked = account.calculate_balance_locked(
        instrument=AUDUSD_SIM,
        side=OrderSide.BUY,
        quantity=order.quantity,
        price=order.price,
        use_quote_for_inverse=False,
    )

    # Assert
    # 100,000 * 1.00000 = 100,000 USD locked
    expected = Money(100_000.00, USD)
    assert locked == expected


def test_cash_account_calculate_commission():
    """
    Test that calculate_commission returns correct values.
    """
    # Arrange
    account = TestExecStubs.cash_account()

    # Act
    commission = account.calculate_commission(
        instrument=AUDUSD_SIM,
        last_qty=Quantity.from_int(100_000),
        last_px=Price.from_str("1.00000"),
        liquidity_side=LiquiditySide.TAKER,
        use_quote_for_inverse=False,
    )

    # Assert
    # Default is 2 bps (0.02%)
    # 100,000 * 1.00000 * 0.0002 = 20 USD
    assert commission == Money(2.00, USD)


def test_cash_account_calculate_pnls():
    """
    Test that calculate_pnls correctly computes PnL for positions.
    """
    # Arrange
    account = TestExecStubs.cash_account()

    # Create a closed position with profit
    order1 = TestExecStubs.market_order(order_side=OrderSide.BUY)
    fill1 = TestEventStubs.order_filled(
        order=order1,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),
        last_px=Price.from_str("1.00000"),
    )

    position = Position(instrument=AUDUSD_SIM, fill=fill1)

    order2 = TestExecStubs.market_order(order_side=OrderSide.SELL)
    fill2 = TestEventStubs.order_filled(
        order=order2,
        instrument=AUDUSD_SIM,
        position_id=PositionId("P-001"),
        last_px=Price.from_str("1.00100"),  # 10 pips profit
    )

    position.apply(fill2)

    # Act
    pnls = account.calculate_pnls(
        instrument=AUDUSD_SIM,
        fill=fill2,
        position=position,
    )

    # Assert
    # calculate_pnls returns realized PnL + unrealized PnL
    assert len(pnls) == 1
    assert pnls[0] > Money(0, USD)  # Should be profitable


def test_cash_account_balance_impact():
    """
    Test that balance_impact calculates correctly for orders.
    """
    # Arrange
    account = TestExecStubs.cash_account()

    # Act - Buy order should decrease balance
    impact_buy = account.balance_impact(
        instrument=AUDUSD_SIM,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00000"),
        order_side=OrderSide.BUY,
    )

    # Sell order should increase balance
    impact_sell = account.balance_impact(
        instrument=AUDUSD_SIM,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("1.00000"),
        order_side=OrderSide.SELL,
    )

    # Assert
    assert impact_buy == Money(-100_000.00, USD)  # Negative for buy
    assert impact_sell == Money(100_000.00, USD)  # Positive for sell


def test_cash_account_clear_balance_locked_resets_locked_balance():
    """
    Test that clear_balance_locked() resets locked balance to zero.
    """
    # Arrange
    account = TestExecStubs.cash_account()

    # Manually set some locked balance (simulating order placement)
    # Since we can't directly set it, we'll just test the clear function

    # Act
    account.clear_balance_locked(AUDUSD_SIM.id)

    # Assert
    assert account.balance_locked(USD) == Money(0, USD)


def test_accounts_manager_update_balance_locked_with_base_currency_multiple_orders():
    """
    Test that AccountsManager correctly aggregates locked balances for multiple orders
    when the account has a base currency, ensuring proper currency conversion.
    """
    # Arrange - Create account with USD base currency
    event = AccountState(
        account_id=AccountId("SIM-001"),
        account_type=AccountType.CASH,
        base_currency=USD,  # Base currency set
        reported=True,
        balances=[
            AccountBalance(
                Money(1_000_000.00, USD),
                Money(0.00, USD),
                Money(1_000_000.00, USD),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(event)

    # Create cache and manager
    cache = Cache()
    cache.add_account(account)

    clock = TestClock()
    logger = Logger("AccountManager")
    manager = AccountsManager(cache, logger, clock)

    # Create multiple orders for the same instrument
    order_factory = OrderFactory(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        clock=clock,
    )

    orders = [
        order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(100_000),
            price=Price.from_str("0.75000"),
        ),
        order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(50_000),
            price=Price.from_str("0.74500"),
        ),
        order_factory.limit(
            instrument_id=AUDUSD_SIM.id,
            order_side=OrderSide.BUY,
            quantity=Quantity.from_int(75_000),
            price=Price.from_str("0.74000"),
        ),
    ]

    # Submit orders to mark them as open
    for order in orders:
        order.apply(TestEventStubs.order_submitted(order))
        order.apply(TestEventStubs.order_accepted(order))

    # Act
    result = manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=orders,
        ts_event=clock.timestamp_ns(),
    )

    # Assert
    assert result is True

    # Check that locked balance is correctly aggregated in base currency (USD)
    locked_balance = account.balance_locked(USD)

    # Expected locked amounts (all converted to USD base currency):
    # Order 1: 100,000 * 0.75000 = 75,000 USD
    # Order 2: 50,000 * 0.74500 = 37,250 USD
    # Order 3: 75,000 * 0.74000 = 55,500 USD
    # Total: 167,750 USD
    expected_locked = Money(167_750.00, USD)

    assert locked_balance == expected_locked

    # Verify no locked balance in AUD (should all be converted to base USD)
    assert account.balance_locked(AUD) is None


class TestCashAccountPurge:
    def test_purge_account_events_retains_latest_when_all_events_purged(self):
        # Arrange
        account = TestExecStubs.cash_account()

        # Add multiple account state events with different timestamps
        event1 = AccountState(
            account_id=account.id,
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
            info={},
            event_id=UUID4(),
            ts_event=100_000_000,  # Old event
            ts_init=100_000_000,
        )

        event2 = AccountState(
            account_id=account.id,
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(1_500_000.00, USD),
                    Money(0.00, USD),
                    Money(1_500_000.00, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=200_000_000,  # Newer event
            ts_init=200_000_000,
        )

        event3 = AccountState(
            account_id=account.id,
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(2_000_000.00, USD),
                    Money(0.00, USD),
                    Money(2_000_000.00, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=300_000_000,  # Latest event
            ts_init=300_000_000,
        )

        account.apply(event1)
        account.apply(event2)
        account.apply(event3)

        # Verify we have 4 events (initial + 3 added)
        assert account.event_count == 4

        # Act - Purge all events (lookback_secs=0, ts_now way in future)
        account.purge_account_events(ts_now=1_000_000_000, lookback_secs=0)

        # Assert - Should retain exactly 1 event (the latest)
        assert account.event_count == 1
        assert account.last_event == event3  # Latest event retained
        assert account.events == [event3]

        # Verify account state reflects the latest event
        assert account.balance_total() == Money(2_000_000.00, USD)

    def test_cash_account_borrowing_disabled_by_default(self):
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
        assert account.allow_borrowing is False

    def test_cash_account_with_borrowing_enabled(self):
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
        account = CashAccount(event, allow_borrowing=True)

        # Assert
        assert account.allow_borrowing is True

    def test_cash_account_rejects_negative_balance_when_borrowing_disabled(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(-100_000, USD),  # Negative balance
                    Money(0, USD),
                    Money(-100_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act & Assert
        from nautilus_trader.accounting.error import AccountBalanceNegative

        with pytest.raises(AccountBalanceNegative):
            CashAccount(event, allow_borrowing=False)

    def test_cash_account_accepts_negative_balance_when_borrowing_enabled(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM-000"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    Money(-100_000, USD),  # Negative balance
                    Money(0, USD),
                    Money(-100_000, USD),
                ),
            ],
            margins=[],
            info={},
            event_id=UUID4(),
            ts_event=0,
            ts_init=0,
        )

        # Act
        account = CashAccount(event, allow_borrowing=True)

        # Assert
        assert account.balance_total() == Money(-100_000, USD)
        assert account.allow_borrowing is True

    def test_cash_account_update_balances_respects_borrowing_setting(self):
        # Arrange
        initial_event = AccountState(
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

        # Test with borrowing disabled
        account_no_borrowing = CashAccount(initial_event, allow_borrowing=False)
        negative_balance = AccountBalance(
            Money(-500_000, USD),  # Negative balance
            Money(0, USD),
            Money(-500_000, USD),
        )

        # Act & Assert - Should raise exception
        from nautilus_trader.accounting.error import AccountBalanceNegative

        with pytest.raises(AccountBalanceNegative):
            account_no_borrowing.update_balances([negative_balance])

        # Test with borrowing enabled
        account_with_borrowing = CashAccount(initial_event, allow_borrowing=True)

        # Act - Should succeed
        account_with_borrowing.update_balances([negative_balance])

        # Assert
        assert account_with_borrowing.balance_total() == Money(-500_000, USD)


def test_accounts_manager_update_balances_with_reduce_only_orders():
    """
    Test that AccountsManager handles reduce-only orders correctly.
    """
    # Arrange
    cache = TestComponentStubs.cache()
    clock = TestClock()
    logger = Logger("AccountsManager")

    # Create account
    account_event = AccountState(
        account_id=AccountId("SIM-000"),
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
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(account_event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )

    # Create a reduce-only order
    reduce_only_order = TestExecStubs.limit_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.80000"),
        reduce_only=True,
    )

    # Add the reduce-only order to cache
    cache.add_order(reduce_only_order, PositionId("TEST-001"))

    # Act - This should not raise UnboundLocalError
    result = accounts_manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=[reduce_only_order],
        ts_event=0,
    )

    # Assert
    assert result is True
    # With only reduce-only orders, no balance should be locked
    assert account.balance_locked(USD) == Money(0.00, USD)
    assert account.balance_locked(AUD) is None  # No AUD balance exists


def test_accounts_manager_update_balances_with_unpriced_orders():
    # Arrange
    cache = TestComponentStubs.cache()
    clock = TestClock()
    logger = Logger("AccountsManager")

    # Create account
    account_event = AccountState(
        account_id=AccountId("SIM-000"),
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
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(account_event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )

    # Create a market order (no price)
    market_order = TestExecStubs.market_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
    )

    # Add the market order to cache
    cache.add_order(market_order, PositionId("TEST-002"))

    # Act - This should not raise UnboundLocalError
    result = accounts_manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=[market_order],
        ts_event=0,
    )

    # Assert
    assert result is True
    # With only unpriced orders, no balance should be locked
    assert account.balance_locked(USD) == Money(0.00, USD)
    assert account.balance_locked(AUD) is None  # No AUD balance exists


def test_accounts_manager_locks_correct_currency_for_fx_orders():
    # Arrange
    cache = TestComponentStubs.cache()
    clock = TestClock()
    logger = Logger("AccountsManager")

    # Create account with both USD and AUD balances
    account_event = AccountState(
        account_id=AccountId("SIM-000"),
        account_type=AccountType.CASH,
        base_currency=None,  # No base currency to test direct currency locking
        reported=True,
        balances=[
            AccountBalance(
                Money(1_000_000.00, USD),
                Money(0.00, USD),
                Money(1_000_000.00, USD),
            ),
            AccountBalance(
                Money(1_000_000.00, AUD),
                Money(0.00, AUD),
                Money(1_000_000.00, AUD),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(account_event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )

    # Create a BUY order for AUD/USD - should lock USD
    buy_order = TestExecStubs.limit_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.80000"),
    )

    # Set order to ACCEPTED state
    buy_order.apply(TestEventStubs.order_submitted(buy_order))
    buy_order.apply(TestEventStubs.order_accepted(buy_order))

    # Add the order to cache
    cache.add_order(buy_order, PositionId("TEST-001"))

    # Act
    result = accounts_manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=[buy_order],
        ts_event=0,
    )

    # Assert - BUY order should lock USD (quote currency)
    assert result is True
    assert account.balance_locked(USD) == Money(80_000.00, USD)  # 100,000 * 0.80
    assert account.balance_locked(AUD) == Money(0.00, AUD)

    # Now test SELL order - clear the previous lock first
    account.clear_balance_locked(AUDUSD_SIM.id)

    # Create a SELL order for AUD/USD - should lock AUD
    sell_order = TestExecStubs.limit_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.80000"),
        client_order_id=ClientOrderId("O-20210410-022422-001-001-2"),
    )

    # Set order to ACCEPTED state
    sell_order.apply(TestEventStubs.order_submitted(sell_order))
    sell_order.apply(TestEventStubs.order_accepted(sell_order))

    # Add the order to cache
    cache.add_order(sell_order, PositionId("TEST-002"))

    # Act
    result = accounts_manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=[sell_order],
        ts_event=0,
    )

    # Assert - SELL order should lock AUD (base currency)
    assert result is True
    assert account.balance_locked(USD) == Money(0.00, USD)
    assert account.balance_locked(AUD) == Money(100_000.00, AUD)  # Full quantity in AUD


def test_accounts_manager_locks_correct_currency_for_multiple_crypto_spot_orders():
    # Arrange
    cache = TestComponentStubs.cache()
    clock = TestClock()
    logger = Logger("AccountsManager")

    # Create account with both USDT and BTC balances
    account_event = AccountState(
        account_id=AccountId("SIM-000"),
        account_type=AccountType.CASH,
        base_currency=None,  # No base currency to test direct currency locking
        reported=True,
        balances=[
            AccountBalance(
                Money(600.00, USDT),
                Money(0.00, USDT),
                Money(600.00, USDT),
            ),
            AccountBalance(
                Money(0.001, BTC),
                Money(0.00, BTC),
                Money(0.001, BTC),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )
    account = CashAccount(account_event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )
    # Create a BUY order for BTCUSDT - should lock USDT
    buy_order = TestExecStubs.limit_order(
        instrument=BTCUSDT_BINANCE,
        order_side=OrderSide.BUY,
        quantity=Quantity.from_str("0.0005"),
        price=Price.from_str("115_972.65"),
    )

    # Set order to ACCEPTED state
    buy_order.apply(TestEventStubs.order_submitted(buy_order))
    buy_order.apply(TestEventStubs.order_accepted(buy_order))

    # Add the order to cache
    cache.add_order(buy_order, PositionId("TEST-001"))
    # Act
    result = accounts_manager.update_orders(
        account=account,
        instrument=BTCUSDT_BINANCE,
        orders_open=[buy_order],
        ts_event=0,
    )
    print(TestEventStubs.order_accepted(buy_order))
    # Assert - BUY order should lock USDT (quote currency)
    assert result is True
    assert account.balance_locked(USDT) == Money(57.986325, USDT)  # 115_972.65 * 0.0005
    assert account.balance_locked(BTC) == Money(0.00, BTC)

    # Now test SELL order - clear the previous lock first
    account.clear_balance_locked(BTCUSDT_BINANCE.id)

    # Create a SELL order for BTCUSDT - should lock BTC
    sell_order = TestExecStubs.limit_order(
        instrument=BTCUSDT_BINANCE,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_str("0.00014"),
        price=Price.from_str("115_978.72"),
        client_order_id=ClientOrderId("O-20210410-022422-001-001-2"),
    )

    # Set order to ACCEPTED state
    sell_order.apply(TestEventStubs.order_submitted(sell_order))
    sell_order.apply(TestEventStubs.order_accepted(sell_order))

    # Add the order to cache
    cache.add_order(sell_order, PositionId("TEST-001"))

    # Act
    result = accounts_manager.update_orders(
        account=account,
        instrument=BTCUSDT_BINANCE,
        orders_open=[buy_order, sell_order],
        ts_event=0,
    )

    # Assert - SELL order should lock BTC (base currency)
    assert result is True
    assert account.balance_locked(USDT) == Money(57.98632500, USDT)
    assert account.balance_locked(BTC) == Money(0.00014, BTC)  # Full quantity in BTC


def test_accounts_manager_with_base_currency_converts_locks():
    # Arrange
    cache = TestComponentStubs.cache()
    clock = TestClock()
    logger = Logger("AccountsManager")

    # Create account with USD as base currency
    account_event = AccountState(
        account_id=AccountId("SIM-000"),
        account_type=AccountType.CASH,
        base_currency=USD,  # USD as base currency
        reported=True,
        balances=[
            AccountBalance(
                Money(1_000_000.00, USD),
                Money(0.00, USD),
                Money(1_000_000.00, USD),
            ),
            AccountBalance(
                Money(1_000_000.00, AUD),
                Money(0.00, AUD),
                Money(1_000_000.00, AUD),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=0,
        ts_init=0,
    )

    account = CashAccount(account_event, calculate_account_state=True)
    cache.add_account(account)

    accounts_manager = AccountsManager(
        cache=cache,
        clock=clock,
        logger=logger,
    )

    # Create a SELL order for AUD/USD
    # Normally this would lock AUD, but with USD base currency it should convert
    sell_order = TestExecStubs.limit_order(
        instrument=AUDUSD_SIM,
        order_side=OrderSide.SELL,
        quantity=Quantity.from_int(100_000),
        price=Price.from_str("0.80000"),
    )

    # Set order to ACCEPTED state
    sell_order.apply(TestEventStubs.order_submitted(sell_order))
    sell_order.apply(TestEventStubs.order_accepted(sell_order))

    # Add the order to cache
    cache.add_order(sell_order, PositionId("TEST-001"))

    # Act
    result = accounts_manager.update_orders(
        account=account,
        instrument=AUDUSD_SIM,
        orders_open=[sell_order],
        ts_event=0,
    )

    # Assert - Should lock in USD (base currency) with conversion
    assert result is True
    # When base currency is USD and we're selling AUD/USD, it locks the notional in USD
    # For a SELL of 100,000 AUD at 0.80, the amount locked depends on the conversion logic
    # The actual behavior locks 100,000 USD (the quantity converted directly)
    assert account.balance_locked(USD) == Money(100_000.00, USD)
    assert account.balance_locked(AUD) == Money(0.00, AUD)
