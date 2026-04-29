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

from datetime import UTC
from datetime import datetime
from types import SimpleNamespace

import pandas as pd
import pytest

from nautilus_trader.accounting.accounts.cash import CashAccount
from nautilus_trader.analysis import CAGR
from nautilus_trader.analysis import CalmarRatio
from nautilus_trader.analysis import MaxDrawdown
from nautilus_trader.analysis import ReturnsAverage
from nautilus_trader.analysis import SharpeRatio
from nautilus_trader.analysis.analyzer import PortfolioAnalyzer
from nautilus_trader.analysis.analyzer import _is_pyo3_statistic
from nautilus_trader.common.component import TestClock
from nautilus_trader.common.factories import OrderFactory
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.model.currencies import AUD
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import PositionId
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.model.position import Position
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.events import TestEventStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


class TestPortfolioAnalyzer:
    def setup(self):
        # Fixture Setup
        self.analyzer = PortfolioAnalyzer()
        self.order_factory = OrderFactory(
            trader_id=TestIdStubs.trader_id(),
            strategy_id=TestIdStubs.strategy_id(),
            clock=TestClock(),
        )

    def test_register_statistic(self):
        # Arrange
        stat = SharpeRatio(period=365)

        # Act
        self.analyzer.register_statistic(stat)

        # Assert
        assert stat.name == "Sharpe Ratio (365 days)"
        assert self.analyzer.statistic(stat.name) == stat

    def test_deregister_statistic(self):
        # Arrange
        stat = SharpeRatio(period=365)
        self.analyzer.register_statistic(stat)

        # Act
        self.analyzer.deregister_statistic(stat)

        # Assert
        assert stat.name == "Sharpe Ratio (365 days)"
        assert self.analyzer.statistic(stat.name) is None

    def test_deregister_statistics(self):
        # Arrange
        stat = SharpeRatio(period=365)
        self.analyzer.register_statistic(stat)

        # Act
        self.analyzer.deregister_statistics()

        # Assert
        assert self.analyzer.statistic("Sharpe Ratio (252 days)") is None

    def test_get_realized_pnls_when_no_data_returns_none(self):
        # Arrange, Act
        result = self.analyzer.realized_pnls()

        # Assert
        assert result is None

    def test_get_realized_pnls_with_currency_when_no_data_returns_none(self):
        # Arrange, Act
        result = self.analyzer.realized_pnls(AUD)

        # Assert
        assert result is None

    def test_is_pyo3_statistic_matches_exact_module(self):
        # Arrange
        stat = _FakePyo3Statistic()

        # Act, Assert
        assert _is_pyo3_statistic(stat)

    def test_is_pyo3_statistic_matches_pyo3_submodule(self):
        # Arrange
        stat = _FakePyo3SubmoduleStatistic()

        # Act, Assert
        assert _is_pyo3_statistic(stat)

    def test_is_pyo3_statistic_rejects_lookalike_module(self):
        # Arrange
        stat = _FakeLookalikePyo3Statistic()

        # Act, Assert
        assert not _is_pyo3_statistic(stat)

    def test_get_performance_stats_pnls_converts_pyo3_statistics_input(self):
        # Arrange
        stat = _RecordingPyo3Statistic()
        self.analyzer.register_statistic(stat)
        self.analyzer.add_trade(PositionId("P-1"), Money(10, USD))

        # Act
        stats = self.analyzer.get_performance_stats_pnls(currency=USD)

        # Assert
        assert stats[stat.name] == 1.0
        assert stat.last_realized_pnls_input == [10.0]

    def test_get_performance_stats_returns_converts_pyo3_statistics_input(self):
        # Arrange
        stat = _RecordingPyo3Statistic()
        self.analyzer.register_statistic(stat)
        self.analyzer.add_position_return(datetime(year=2010, month=1, day=1, tzinfo=UTC), 0.05)

        # Act
        stats = self.analyzer.get_performance_stats_returns()

        # Assert
        assert stats[stat.name] == 1.0
        assert stat.last_returns_input is not None
        assert isinstance(stat.last_returns_input, dict)
        assert list(stat.last_returns_input.values()) == [0.05]

    @pytest.mark.parametrize("cls", [MaxDrawdown, CAGR, CalmarRatio])
    def test_returns_based_pyo3_statistics_skip_pnl_and_position_calls(self, cls):
        # Returns-based pyo3 statistics must implement the full PortfolioStatistic
        # surface (returning None for non-applicable inputs) so registering them
        # does not raise AttributeError during backtest end.
        self.analyzer.register_statistic(cls())
        self.analyzer.add_trade(PositionId("P-1"), Money(10, USD))

        pnl_stats = self.analyzer.get_performance_stats_pnls(currency=USD)
        general_stats = self.analyzer.get_performance_stats_general()

        assert cls().name not in pnl_stats
        assert cls().name not in general_stats

    def test_analyzer_tracks_position_returns(self):
        # Arrange
        t1 = datetime(year=2010, month=1, day=1, tzinfo=UTC)
        t2 = datetime(year=2010, month=1, day=2, tzinfo=UTC)
        t3 = datetime(year=2010, month=1, day=3, tzinfo=UTC)
        t4 = datetime(year=2010, month=1, day=4, tzinfo=UTC)
        t5 = datetime(year=2010, month=1, day=5, tzinfo=UTC)
        t6 = datetime(year=2010, month=1, day=6, tzinfo=UTC)
        t7 = datetime(year=2010, month=1, day=7, tzinfo=UTC)
        t8 = datetime(year=2010, month=1, day=8, tzinfo=UTC)
        t9 = datetime(year=2010, month=1, day=9, tzinfo=UTC)
        t10 = datetime(year=2010, month=1, day=10, tzinfo=UTC)

        # Act
        self.analyzer.add_position_return(t1, 0.05)
        self.analyzer.add_position_return(t2, -0.10)
        self.analyzer.add_position_return(t3, 0.10)
        self.analyzer.add_position_return(t4, -0.21)
        self.analyzer.add_position_return(t5, 0.22)
        self.analyzer.add_position_return(t6, -0.23)
        self.analyzer.add_position_return(t7, 0.24)
        self.analyzer.add_position_return(t8, -0.25)
        self.analyzer.add_position_return(t9, 0.26)
        self.analyzer.add_position_return(t10, -0.10)
        self.analyzer.add_position_return(t10, -0.10)
        result = self.analyzer.position_returns()

        # Assert
        assert len(result) == 10
        assert self.analyzer.portfolio_returns().empty
        pd.testing.assert_series_equal(self.analyzer.returns(), result)

    def test_get_realized_pnls_when_all_flat_positions_returns_expected_series(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order2 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        order3 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        order4 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.SELL,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=TestIdStubs.strategy_id(),
            last_px=Price.from_str("1.00000"),
        )

        fill2 = TestEventStubs.order_filled(
            order2,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=TestIdStubs.strategy_id(),
            last_px=Price.from_str("1.00010"),
        )

        fill3 = TestEventStubs.order_filled(
            order3,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-2"),
            strategy_id=TestIdStubs.strategy_id(),
            last_px=Price.from_str("1.00000"),
        )

        fill4 = TestEventStubs.order_filled(
            order4,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-2"),
            strategy_id=TestIdStubs.strategy_id(),
            last_px=Price.from_str("1.00020"),
        )

        position1 = Position(instrument=AUDUSD_SIM, fill=fill1)
        position1.apply(fill2)

        position2 = Position(instrument=AUDUSD_SIM, fill=fill3)
        position2.apply(fill4)

        self.analyzer.add_positions([position1, position2])

        # Act
        result = self.analyzer.realized_pnls(USD)

        # Assert
        assert self.analyzer.currencies == []
        assert len(result) == 2
        assert result["P-1"] == 6.0
        assert result["P-2"] == 16.0

    def test_add_positions_skips_empty_shell_positions_with_zero_ts_closed(self):
        # Arrange
        order1 = self.order_factory.market(
            AUDUSD_SIM.id,
            OrderSide.BUY,
            Quantity.from_int(100_000),
        )

        fill1 = TestEventStubs.order_filled(
            order1,
            instrument=AUDUSD_SIM,
            position_id=PositionId("P-1"),
            strategy_id=TestIdStubs.strategy_id(),
            last_px=Price.from_str("1.00000"),
        )

        position = Position(instrument=AUDUSD_SIM, fill=fill1)

        # Purge the fill to create an empty shell with ts_closed = 0
        position.purge_events_for_order(order1.client_order_id)

        # Act
        self.analyzer.add_positions([position])
        returns = self.analyzer.returns()

        # Assert: no returns should be added for empty shell position
        assert len(returns) == 0

    def test_calculate_statistics_separates_position_and_portfolio_returns(self):
        # Arrange
        account = _create_cash_account(
            [
                ("2024-01-01", 1_000.0),
                ("2024-01-10", 1_050.0),
                ("2024-01-31", 1_100.0),
            ],
        )
        positions = [
            _create_closed_position(
                position_id="P-1",
                realized_pnl=100.0,
                realized_return=0.30,
                ts_closed="2024-01-31",
            ),
        ]

        # Act
        self.analyzer.calculate_statistics(account, positions)
        position_returns = self.analyzer.position_returns()
        portfolio_returns = self.analyzer.portfolio_returns()

        # Assert
        assert position_returns.loc[pd.Timestamp("2024-01-31", tz="UTC")] == pytest.approx(0.30)
        assert portfolio_returns.loc[pd.Timestamp("2024-01-10", tz="UTC")] == pytest.approx(0.05)
        assert portfolio_returns.loc[pd.Timestamp("2024-01-11", tz="UTC")] == pytest.approx(0.0)
        pd.testing.assert_series_equal(self.analyzer.returns(), portfolio_returns)

    def test_get_performance_stats_returns_prefers_portfolio_returns(self):
        # Arrange
        self.analyzer.register_statistic(ReturnsAverage())
        account = _create_cash_account(
            [
                ("2024-01-01", 1_000.0),
                ("2024-01-10", 1_050.0),
                ("2024-01-31", 1_100.0),
            ],
        )
        positions = [
            _create_closed_position(
                position_id="P-1",
                realized_pnl=100.0,
                realized_return=0.30,
                ts_closed="2024-01-31",
            ),
        ]

        # Act
        self.analyzer.calculate_statistics(account, positions)
        position_stats = self.analyzer.get_performance_stats_position_returns()
        portfolio_stats = self.analyzer.get_performance_stats_portfolio_returns()
        returns_stats = self.analyzer.get_performance_stats_returns()

        # Assert
        assert position_stats["Average (Return)"] == pytest.approx(0.30)
        assert portfolio_stats["Average (Return)"] == pytest.approx(
            self.analyzer.portfolio_returns().mean(),
        )
        assert returns_stats == portfolio_stats

    def test_get_performance_stats_returns_falls_back_to_position_returns(self):
        # Arrange
        self.analyzer.register_statistic(ReturnsAverage())
        account = _create_cash_account([("2024-01-31", 1_000.0)])
        positions = [
            _create_closed_position(
                position_id="P-1",
                realized_pnl=100.0,
                realized_return=0.30,
                ts_closed="2024-01-31",
            ),
        ]

        # Act
        self.analyzer.calculate_statistics(account, positions)
        position_stats = self.analyzer.get_performance_stats_position_returns()
        returns_stats = self.analyzer.get_performance_stats_returns()

        # Assert
        assert self.analyzer.portfolio_returns().empty
        assert position_stats["Average (Return)"] == pytest.approx(0.30)
        assert returns_stats == position_stats

    def test_portfolio_returns_skips_empty_balance_snapshots(self):
        # Arrange
        account_id = TestIdStubs.account_id()
        account = CashAccount(
            _create_cash_account_state(
                account_id=account_id,
                total=1_000.0,
                ts_event=pd.Timestamp("2024-01-01", tz="UTC").value,
            ),
            calculate_account_state=False,
        )
        account.apply(
            AccountState(
                account_id=account_id,
                account_type=AccountType.CASH,
                base_currency=USD,
                reported=True,
                balances=[],
                margins=[],
                info={},
                event_id=UUID4(),
                ts_event=pd.Timestamp("2024-01-15", tz="UTC").value,
                ts_init=pd.Timestamp("2024-01-15", tz="UTC").value,
            ),
        )
        account.apply(
            _create_cash_account_state(
                account_id=account_id,
                total=1_050.0,
                ts_event=pd.Timestamp("2024-01-31", tz="UTC").value,
            ),
        )

        # Act
        self.analyzer.calculate_statistics(account, [])
        portfolio_returns = self.analyzer.portfolio_returns()

        # Assert
        assert not portfolio_returns.empty
        assert portfolio_returns.loc[pd.Timestamp("2024-01-31", tz="UTC")] == pytest.approx(0.05)

    def test_calculate_statistics_filters_non_finite_portfolio_returns(self):
        # Arrange
        account = _create_cash_account(
            [
                ("2024-01-01", 0.0),
                ("2024-01-10", 1_000.0),
                ("2024-01-31", 1_050.0),
            ],
        )

        # Act
        self.analyzer.calculate_statistics(account, [])
        portfolio_returns = self.analyzer.portfolio_returns()

        # Assert
        assert portfolio_returns.notna().all()
        assert portfolio_returns.loc[pd.Timestamp("2024-01-31", tz="UTC")] == pytest.approx(0.05)


def _create_cash_account(events: list[tuple[str, float]]) -> CashAccount:
    account_id = TestIdStubs.account_id()
    first_date, first_total = events[0]
    account = CashAccount(
        _create_cash_account_state(
            account_id=account_id,
            total=first_total,
            ts_event=pd.Timestamp(first_date, tz="UTC").value,
        ),
        calculate_account_state=False,
    )

    for date_str, total in events[1:]:
        account.apply(
            _create_cash_account_state(
                account_id=account_id,
                total=total,
                ts_event=pd.Timestamp(date_str, tz="UTC").value,
            ),
        )

    return account


def _create_cash_account_state(
    account_id,
    total: float,
    ts_event: int,
) -> AccountState:
    return AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USD,
        reported=True,
        balances=[
            AccountBalance(
                Money(total, USD),
                Money(0, USD),
                Money(total, USD),
            ),
        ],
        margins=[],
        info={},
        event_id=UUID4(),
        ts_event=ts_event,
        ts_init=ts_event,
    )


def _create_closed_position(
    position_id: str,
    realized_pnl: float,
    realized_return: float,
    ts_closed: str,
) -> SimpleNamespace:
    return SimpleNamespace(
        id=PositionId(position_id),
        realized_pnl=Money(realized_pnl, USD),
        realized_return=realized_return,
        ts_closed=pd.Timestamp(ts_closed, tz="UTC").value,
    )


class _RecordingPyo3Statistic:
    __module__ = "nautilus_trader.core.nautilus_pyo3"

    def __init__(self) -> None:
        self.last_realized_pnls_input = None
        self.last_returns_input = None

    @property
    def name(self) -> str:
        return "Recording Pyo3 Statistic"

    def calculate_from_realized_pnls(self, realized_pnls):
        self.last_realized_pnls_input = realized_pnls
        return 1.0

    def calculate_from_returns(self, returns):
        self.last_returns_input = returns
        return 1.0

    def calculate_from_positions(self, positions):
        return None


class _FakePyo3Statistic:
    __module__ = "nautilus_trader.core.nautilus_pyo3"


class _FakePyo3SubmoduleStatistic:
    __module__ = "nautilus_trader.core.nautilus_pyo3.analysis"


class _FakeLookalikePyo3Statistic:
    __module__ = "nautilus_trader.core.nautilus_pyo3_fake"
