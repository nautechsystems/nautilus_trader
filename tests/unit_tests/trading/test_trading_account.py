# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2021 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.cache.cache import Cache
from nautilus_trader.cache.database import BypassCacheDatabase
from nautilus_trader.common.clock import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.common.logging import Logger
from nautilus_trader.core.uuid import uuid4
from nautilus_trader.execution.engine import ExecutionEngine
from nautilus_trader.model.currencies import ADA
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import ETH
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TraderId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.trading.account import Account
from nautilus_trader.trading.portfolio import Portfolio
from tests.test_kit.providers import TestInstrumentProvider
from tests.test_kit.stubs import TestStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
USDJPY_SIM = TestInstrumentProvider.default_fx_ccy("USD/JPY")
ADABTC_BINANCE = TestInstrumentProvider.adabtc_binance()
BTCUSDT_BINANCE = TestInstrumentProvider.btcusdt_binance()


class TestAccount:
    def setup(self):
        # Fixture Setup
        clock = TestClock()
        logger = Logger(clock)
        trader_id = TraderId("TESTER-000")
        self.order_factory = OrderFactory(
            trader_id=trader_id,
            strategy_id=StrategyId("S-001"),
            clock=TestClock(),
        )

        cache_db = BypassCacheDatabase(
            trader_id=trader_id,
            logger=logger,
        )

        cache = Cache(
            database=cache_db,
            logger=logger,
        )

        self.portfolio = Portfolio(
            cache=cache,
            clock=clock,
            logger=logger,
        )

        self.exec_engine = ExecutionEngine(
            portfolio=self.portfolio,
            cache=cache,
            clock=clock,
            logger=logger,
        )

    def test_instantiated_accounts_basic_properties(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event)

        # Prepare components
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Assert
        assert account.id == AccountId("SIM", "001")
        assert str(account) == "Account(id=SIM-001)"
        assert repr(account) == "Account(id=SIM-001)"
        assert isinstance(hash(account), int)
        assert account == account
        assert not account != account

    def test_instantiate_single_asset_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event)

        # Prepare components
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

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
        assert account.unrealized_pnl() == Money(0, USD)
        assert account.equity() == Money(1_000_000, USD)
        assert account.initial_margins() == {}
        assert account.maint_margins() == {}
        assert account.initial_margin() is None
        assert account.maint_margin() is None

    def test_instantiate_multi_asset_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event)

        # Prepare components
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Assert
        assert account.id == AccountId("SIM", "001")
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
        assert account.unrealized_pnl(BTC) == Money(0.00000000, BTC)
        assert account.unrealized_pnl(ETH) == Money(0.00000000, ETH)
        assert account.equity(BTC) == Money(10.00000000, BTC)
        assert account.equity(ETH) == Money(20.00000000, ETH)
        assert account.initial_margins() == {}
        assert account.maint_margins() == {}
        assert account.initial_margin(BTC) is None
        assert account.initial_margin(ETH) is None
        assert account.maint_margin(BTC) is None
        assert account.maint_margin(ETH) is None

    def test_apply_given_new_state_event_updates_correctly(self):
        # Arrange
        event1 = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event1)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        event2 = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(9.00000000, BTC),
                    Money(0.50000000, BTC),
                    Money(8.50000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
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

    def test_update_initial_margin(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        margin = Money(0.00100000, BTC)

        # Act
        account.update_initial_margin(margin)

        # Assert
        assert account.initial_margin(BTC) == margin
        assert account.initial_margins() == {BTC: margin}

    def test_update_maint_margin(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        # Act
        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        margin = Money(0.00050000, BTC)

        # Act
        account.update_maint_margin(margin)

        # Assert
        assert account.maint_margin(BTC) == margin
        assert account.maint_margins() == {BTC: margin}

    def test_unrealized_pnl_with_single_asset_account_when_no_open_positions_returns_zero(
        self,
    ):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000, USD),
                    Money(0, USD),
                    Money(1_000_000, USD),
                ),
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.unrealized_pnl()

        # Assert
        assert result == Money(0, USD)

    def test_unrealized_pnl_with_multi_asset_account_when_no_open_positions_returns_zero(
        self,
    ):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.unrealized_pnl(BTC)

        # Assert
        assert result == Money(0.00000000, BTC)

    def test_equity_with_single_asset_account_no_default_returns_none(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(100000.00, USD),
                    Money(0.00, USD),
                    Money(100000.00, USD),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.equity(BTC)

        # Assert
        assert result is None

    def test_equity_with_single_asset_account_returns_expected_money(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(100000.00, USD),
                    Money(0.00, USD),
                    Money(100000.00, USD),
                ),
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.equity()

        # Assert
        assert result == Money(100000.00, USD)

    def test_equity_with_multi_asset_account_returns_expected_money(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result = account.equity(BTC)

        # Assert
        assert result == Money(10.00000000, BTC)

    def test_margin_available_for_single_asset_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(100000.00, USD),
                    Money(0.00, USD),
                    Money(100000.00, USD),
                ),
            ],
            info={},
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result1 = account.margin_available()
        account.update_initial_margin(Money(500.00, USD))
        result2 = account.margin_available()
        account.update_maint_margin(Money(1000.00, USD))
        result3 = account.margin_available()

        # Assert
        assert result1 == Money(100000.00, USD)
        assert result2 == Money(99500.00, USD)
        assert result3 == Money(98500.00, USD)

    def test_margin_available_for_multi_asset_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        # Act
        result1 = account.margin_available(BTC)
        account.update_initial_margin(Money(0.00010000, BTC))
        result2 = account.margin_available(BTC)
        account.update_maint_margin(Money(0.00020000, BTC))
        result3 = account.margin_available(BTC)
        result4 = account.margin_available(ETH)

        # Assert
        assert result1 == Money(10.00000000, BTC)
        assert result2 == Money(9.99990000, BTC)
        assert result3 == Money(9.99970000, BTC)
        assert result4 == Money(20.00000000, ETH)

    def test_calculate_pnls_for_single_currency_cash_account(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=USD,
            reported=True,
            balances=[
                AccountBalance(
                    USD,
                    Money(1_000_000.00, USD),
                    Money(0.00, USD),
                    Money(1_000_000.00, USD),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        order = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(1_000_000),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("0.80000"),
        )

        position = Position(AUDUSD_SIM, fill)

        # Act
        result = account.calculate_pnls(
            instrument=AUDUSD_SIM,
            position=position,
            fill=fill,
        )

        # Assert
        assert result == [Money(1000000.00, AUD), Money(-800000.00, USD)]

    def test_calculate_pnls_for_multi_currency_cash_account_btcusdt(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(10.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(10.00000000, BTC),
                ),
                AccountBalance(
                    ETH,
                    Money(20.00000000, ETH),
                    Money(0.00000000, ETH),
                    Money(20.00000000, ETH),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        order = self.order_factory.market(
            BTCUSDT_BINANCE.id,
            OrderSide.SELL,
            Quantity.from_str("0.50000000"),
        )

        fill = TestStubs.event_order_filled(
            order,
            instrument=BTCUSDT_BINANCE,
            position_id=PositionId("P-123456"),
            strategy_id=StrategyId("S-001"),
            last_px=Price.from_str("45500.00"),
        )

        position = Position(BTCUSDT_BINANCE, fill)

        # Act
        result = account.calculate_pnls(
            instrument=BTCUSDT_BINANCE,
            position=position,
            fill=fill,
        )

        # Assert
        assert result == [Money(-0.50000000, BTC), Money(22750.00000000, USDT)]

    def test_calculate_pnls_for_multi_currency_cash_account_adabtc(self):
        # Arrange
        event = AccountState(
            account_id=AccountId("SIM", "001"),
            account_type=AccountType.CASH,
            base_currency=None,  # Multi-currency
            reported=True,
            balances=[
                AccountBalance(
                    BTC,
                    Money(1.00000000, BTC),
                    Money(0.00000000, BTC),
                    Money(1.00000000, BTC),
                ),
                AccountBalance(
                    ADA,
                    Money(1000.00000000, ADA),
                    Money(0.00000000, ADA),
                    Money(1000.00000000, ADA),
                ),
            ],
            info={},  # No default currency set
            event_id=uuid4(),
            ts_updated_ns=0,
            timestamp_ns=0,
        )

        account = Account(event)

        # Wire up account to portfolio
        account.register_portfolio(self.portfolio)
        self.portfolio.register_account(account)

        order = self.order_factory.market(
            ADABTC_BINANCE.id,
            OrderSide.BUY,
            Quantity.from_int(100),
        )

        fill = TestStubs.event_order_filled(
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
            position=position,
            fill=fill,
        )

        # Assert
        assert result == [Money(100.000000, ADA), Money(-0.00410000, BTC)]
