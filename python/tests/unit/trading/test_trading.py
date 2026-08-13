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

import datetime as dt
import inspect
from decimal import Decimal

import pytest

import nautilus_trader.model
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.common import ComponentState
from nautilus_trader.common import CustomData
from nautilus_trader.common import QueueCondition
from nautilus_trader.common import QueueState
from nautilus_trader.common import QueueStateChanged
from nautilus_trader.common import Signal
from nautilus_trader.common import SocketState
from nautilus_trader.common import SocketStateChanged
from nautilus_trader.common import SystemChannel
from nautilus_trader.common import TimeEvent
from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import Bar
from nautilus_trader.model import BarType
from nautilus_trader.model import BookAction
from nautilus_trader.model import BookOrder
from nautilus_trader.model import BookType
from nautilus_trader.model import ClientId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import Currency
from nautilus_trader.model import DataType
from nautilus_trader.model import FundingRateUpdate
from nautilus_trader.model import IndexPriceUpdate
from nautilus_trader.model import InstrumentClose
from nautilus_trader.model import InstrumentCloseType
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import InstrumentStatus
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketStatusAction
from nautilus_trader.model import MarkPriceUpdate
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OptionChainSlice
from nautilus_trader.model import OptionGreeks
from nautilus_trader.model import OptionSeriesId
from nautilus_trader.model import OrderAccepted
from nautilus_trader.model import OrderBook
from nautilus_trader.model import OrderBookDelta
from nautilus_trader.model import OrderBookDeltas
from nautilus_trader.model import OrderCanceled
from nautilus_trader.model import OrderCancelRejected
from nautilus_trader.model import OrderDenied
from nautilus_trader.model import OrderEmulated
from nautilus_trader.model import OrderExpired
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderInitialized
from nautilus_trader.model import OrderList
from nautilus_trader.model import OrderListId
from nautilus_trader.model import OrderModifyRejected
from nautilus_trader.model import OrderPendingCancel
from nautilus_trader.model import OrderPendingUpdate
from nautilus_trader.model import OrderRejected
from nautilus_trader.model import OrderReleased
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderTriggered
from nautilus_trader.model import OrderType
from nautilus_trader.model import OrderUpdated
from nautilus_trader.model import Position
from nautilus_trader.model import PositionChanged
from nautilus_trader.model import PositionClosed
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionOpened
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import StrikeRange
from nautilus_trader.model import Symbol
from nautilus_trader.model import SyntheticInstrument
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TradeTick
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId
from nautilus_trader.trading import ForexSession
from nautilus_trader.trading import ImportableStrategyConfig
from nautilus_trader.trading import Strategy
from nautilus_trader.trading import StrategyConfig
from nautilus_trader.trading import fx_local_from_utc
from nautilus_trader.trading import fx_next_end
from nautilus_trader.trading import fx_next_start
from nautilus_trader.trading import fx_prev_end
from nautilus_trader.trading import fx_prev_start
from tests.providers import TestInstrumentProvider
from tests.unit.common.actor import OrderFactoryConfigProbeStrategy
from tests.unit.common.actor import OrderFactoryProbeStrategy
from tests.unit.common.actor import OrderListCacheProbeStrategy
from tests.unit.common.actor import PortfolioHedgedProbeStrategy
from tests.unit.common.actor import PortfolioMultiVenueProbeStrategy
from tests.unit.common.actor import PortfolioPositionProbeStrategy
from tests.unit.common.actor import PortfolioProbeStrategy
from tests.unit.common.actor import TestStrategy


HISTORICAL_REQUEST_DATETIME_CASES = [
    pytest.param("datetime-utc", id="datetime-utc"),
    pytest.param("pandas-timestamp-utc", id="pandas-timestamp-utc"),
    pytest.param("pandas-timestamp-utc-nanos", id="pandas-timestamp-utc-nanos"),
]
DATA_OPERATION_REGISTRATION_ERROR = (
    "Strategy must be registered before publishing, managing synthetics, or requesting data"
)


class HistoricalRequestProbeStrategy(Strategy):
    observed_request_ids = {}
    request_time = dt.datetime(1970, 1, 1, tzinfo=dt.UTC)

    def on_start(self):
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        client_id = ClientId("SIM")
        venue = Venue("SIM")
        bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
        request_time = type(self).request_time

        type(self).observed_request_ids = {
            "data": self.request_data(
                DataType("TestData"),
                client_id,
                start=request_time,
                limit=1,
                params={"kind": "data"},
            ),
            "instrument": self.request_instrument(
                instrument_id,
                start=request_time,
                params={"kind": "instrument"},
            ),
            "instruments": self.request_instruments(
                venue,
                end=request_time,
                params={"kind": "instruments"},
            ),
            "book_snapshot": self.request_book_snapshot(
                instrument_id,
                depth=5,
                params={"kind": "snapshot"},
            ),
            "book_deltas": self.request_book_deltas(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "deltas"},
            ),
            "book_depth": self.request_book_depth(
                instrument_id,
                end=request_time,
                limit=2,
                depth=5,
                params={"kind": "depth"},
            ),
            "quotes": self.request_quotes(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "quotes"},
            ),
            "trades": self.request_trades(
                instrument_id,
                end=request_time,
                limit=1,
                params={"kind": "trades"},
            ),
            "funding_rates": self.request_funding_rates(
                instrument_id,
                start=request_time,
                limit=1,
                params={"kind": "funding-rates"},
            ),
            "bars": self.request_bars(
                bar_type,
                end=request_time,
                limit=1,
                params={"kind": "bars"},
            ),
        }


def test_strategy_default_construction():
    strategy = Strategy()

    assert strategy.trader_id is None
    assert strategy.strategy_id is not None
    assert strategy.state() == ComponentState.PRE_INITIALIZED
    assert strategy.is_ready() is False
    assert strategy.is_running() is False
    assert strategy.is_stopped() is False
    assert strategy.is_disposed() is False
    assert strategy.is_degraded() is False
    assert strategy.is_faulted() is False


def test_strategy_construction_with_config():
    config = StrategyConfig(
        StrategyId("S-001"),
        "001",
        None,
        None,
        False,
        False,
        False,
        100,
        100,
        TimeInForce.GTC,
        False,
        True,
        True,
        True,
        True,
        False,
    )
    strategy = Strategy(config)

    assert strategy.strategy_id == StrategyId("S-001")


def test_strategy_clock_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.clock


def test_strategy_cache_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.cache


def test_strategy_portfolio_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.portfolio


def test_strategy_order_factory_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered with a trader"):
        _ = strategy.order_factory


def test_strategy_order_factory_returns_registered_factory():
    usd = Currency.from_str("USD")
    venue = Venue("SIM")
    OrderFactoryProbeStrategy.observed_order = None
    OrderFactoryProbeStrategy.observed_invalid_order_error = None
    OrderFactoryProbeStrategy.observed_next_client_order_id = None
    OrderFactoryProbeStrategy.observed_client_order_id_count = None
    OrderFactoryProbeStrategy.observed_order_list_id_count = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:OrderFactoryProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        order = OrderFactoryProbeStrategy.observed_order
        assert order is not None
        assert order.order_type == OrderType.MARKET
        assert order.side == OrderSide.BUY
        assert order.quantity == Quantity.from_str("100000")
        assert "GTD not supported for Market orders" in (
            OrderFactoryProbeStrategy.observed_invalid_order_error
        )
        assert OrderFactoryProbeStrategy.observed_next_client_order_id != order.client_order_id
        assert OrderFactoryProbeStrategy.observed_client_order_id_count == 3
        assert OrderFactoryProbeStrategy.observed_order_list_id_count == 0
    finally:
        engine.dispose()


@pytest.mark.parametrize(
    ("use_uuid_client_order_ids", "expected_client_order_id"),
    [
        (False, ClientOrderId("O197001010000000010071")),
        (True, None),
    ],
)
def test_registered_strategy_order_factory_uses_configured_identity_and_id_format(
    use_uuid_client_order_ids,
    expected_client_order_id,
):
    OrderFactoryConfigProbeStrategy.reset()
    usd = Currency.from_str("USD")
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )

    try:
        strategy = OrderFactoryConfigProbeStrategy(
            StrategyConfig(
                strategy_id=StrategyId("FORMAT-007"),
                use_uuid_client_order_ids=use_uuid_client_order_ids,
                use_hyphens_in_client_order_ids=False,
            ),
        )
        engine.add_strategy(strategy)
        engine.run()

        order_factory = OrderFactoryConfigProbeStrategy.observed_factory
        config = OrderFactoryConfigProbeStrategy.observed_config
        client_order_id = OrderFactoryConfigProbeStrategy.observed_client_order_id
        assert order_factory.trader_id == TraderId("TRADER-001")
        assert order_factory.strategy_id == StrategyId("FORMAT-007")
        assert config.use_uuid_client_order_ids is use_uuid_client_order_ids
        assert config.use_hyphens_in_client_order_ids is False

        if expected_client_order_id is None:
            assert len(str(client_order_id)) == 32
            assert "-" not in str(client_order_id)
        else:
            assert client_order_id == expected_client_order_id
    finally:
        engine.dispose()


def test_strategy_can_recover_order_list_id_from_cache():
    usd = Currency.from_str("USD")
    venue = Venue("SIM")
    instrument = TestInstrumentProvider.audusd_sim()
    OrderListCacheProbeStrategy.reset()

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )
    engine.add_instrument(instrument)

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:OrderListCacheProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        order_list = OrderListCacheProbeStrategy.observed_order_list
        order_lists = OrderListCacheProbeStrategy.observed_order_lists
        order_list_id = OrderListCacheProbeStrategy.observed_order_list_id
        client_order_ids = OrderListCacheProbeStrategy.observed_client_order_ids

        assert order_list is not None
        assert order_lists == [order_list]
        assert type(order_list) is OrderList
        assert OrderList.__module__ == "nautilus_trader.model"
        assert isinstance(order_list_id, OrderListId)
        assert order_list.id == order_list_id
        assert order_list.instrument_id == instrument.id
        assert order_list.strategy_id == OrderListCacheProbeStrategy.observed_strategy_id
        assert order_list.client_order_ids() == client_order_ids
        assert order_list.first_client_order_id == client_order_ids[0]
        assert len(order_list) == 3
        assert hash(order_list) == hash(order_list.id)
        assert repr(order_list) == str(order_list)

        returned_ids = order_list.client_order_ids()
        returned_ids.clear()
        assert len(order_list) == 3

        with pytest.raises(AttributeError):
            order_list.id = OrderListId("OL-IMMUTABLE")
    finally:
        engine.dispose()


def test_strategy_portfolio_returns_registered_kernel_portfolio():
    usd = Currency.from_str("USD")
    venue = Venue("SIM")
    PortfolioProbeStrategy.observed_portfolio = None
    PortfolioProbeStrategy.observed_account = None
    PortfolioProbeStrategy.observed_equity_by_venue = None
    PortfolioProbeStrategy.observed_equity_by_account = None
    PortfolioProbeStrategy.observed_initialized = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:PortfolioProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        portfolio = PortfolioProbeStrategy.observed_portfolio
        account = PortfolioProbeStrategy.observed_account
        assert portfolio is not None
        assert account is not None
        assert PortfolioProbeStrategy.observed_initialized is False
        assert account.id == AccountId("SIM-001")
        assert PortfolioProbeStrategy.observed_equity_by_venue[usd] == Money(1_000_000.0, usd)
        assert PortfolioProbeStrategy.observed_equity_by_account[usd] == Money(1_000_000.0, usd)

        assert portfolio.account(account_id=account.id).id == account.id
        assert portfolio.mark_values(account_id=account.id) == {}
        assert portfolio.net_exposures() == {}
        assert portfolio.net_exposures(account_id=account.id) == {}
        assert portfolio.equity(account_id=AccountId("OTHER-001")) == {}

        recorded_snapshots = portfolio.snapshots(account.id)
        assert portfolio.build_snapshot(account.id) is not None
        assert portfolio.snapshots(account.id) == recorded_snapshots

        detached = portfolio.account(account_id=account.id)
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        detached.set_leverage(instrument_id, Decimal(7))
        assert detached.leverage(instrument_id) == Decimal(7)
        assert portfolio.account(account_id=account.id).leverage(instrument_id) != Decimal(
            7,
        )

        for command in (
            "initialize_orders",
            "initialize_positions",
            "reset",
            "update_account",
            "update_order",
            "update_position",
        ):
            assert not hasattr(portfolio, command)

        for old_name in (
            "account_snapshot",
            "is_completely_flat",
            "is_flat",
            "margins_init",
            "margins_maint",
            "recorded_realized_pnls",
        ):
            assert not hasattr(portfolio, old_name)

        mismatched_venue = Venue("OTHER")
        mismatched_queries = (
            lambda: portfolio.account(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.balances_locked(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.instrument_initial_margins(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.instrument_maintenance_margins(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.realized_pnls(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.unrealized_pnls(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.total_pnls(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.net_exposures(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.mark_values(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.equity(
                venue=mismatched_venue,
                account_id=account.id,
            ),
            lambda: portfolio.missing_price_instruments(
                mismatched_venue,
                account_id=account.id,
            ),
        )

        for query in mismatched_queries:
            with pytest.raises(ValueError, match="do not resolve to the same account"):
                query()

        with pytest.raises(ValueError, match="venue or account_id must be provided"):
            portfolio.account()
    finally:
        engine.dispose()


def test_strategy_portfolio_accepts_price_and_target_currency_queries():
    usd = Currency.from_str("USD")
    venue = Venue("SIM")
    PortfolioProbeStrategy.observed_portfolio = None
    PortfolioProbeStrategy.observed_account = None
    PortfolioProbeStrategy.observed_equity_by_venue = None
    PortfolioProbeStrategy.observed_equity_by_account = None
    PortfolioProbeStrategy.observed_initialized = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:PortfolioProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        portfolio = PortfolioProbeStrategy.observed_portfolio
        instrument_id = InstrumentId.from_str("AUD/USD.SIM")
        assert portfolio is not None

        assert portfolio.realized_pnls(venue=venue, target_currency=usd) == {}
        assert portfolio.unrealized_pnls(venue=venue, target_currency=usd) == {}
        assert portfolio.total_pnls(venue=venue, target_currency=usd) == {}
        assert portfolio.net_exposures(venue=venue, target_currency=usd) == {}
        assert portfolio.realized_pnl(instrument_id, target_currency=usd) is None
        assert (
            portfolio.unrealized_pnl(
                instrument_id,
                price=Price.from_str("1.00000"),
                target_currency=usd,
            )
            is None
        )
        assert (
            portfolio.total_pnl(
                instrument_id,
                price=Price.from_str("1.00000"),
                target_currency=usd,
            )
            is None
        )
        assert (
            portfolio.net_exposure(
                instrument_id,
                price=Price.from_str("1.00000"),
                target_currency=usd,
            )
            is None
        )
    finally:
        engine.dispose()


def test_strategy_portfolio_flat_methods_net_hedged_positions():
    usd = Currency.from_str("USD")
    venue = Venue("SIM")
    instrument = TestInstrumentProvider.audusd_sim()
    PortfolioHedgedProbeStrategy.observed_portfolio = None
    PortfolioHedgedProbeStrategy.observed_account = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )
    engine.add_instrument(instrument)
    engine.add_data(
        [
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str("0.90000"),
                ask_price=Price.from_str("0.90002"),
                bid_size=Quantity.from_str("1000000"),
                ask_size=Quantity.from_str("1000000"),
                ts_event=1,
                ts_init=1,
            ),
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str("0.90000"),
                ask_price=Price.from_str("0.90002"),
                bid_size=Quantity.from_str("1000000"),
                ask_size=Quantity.from_str("1000000"),
                ts_event=2,
                ts_init=2,
            ),
        ],
    )

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:PortfolioHedgedProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        portfolio = PortfolioHedgedProbeStrategy.observed_portfolio
        account = PortfolioHedgedProbeStrategy.observed_account
        result = engine.get_result()

        assert portfolio is not None
        assert account is not None
        assert result.total_orders == 2
        assert result.total_positions == 2
        assert portfolio.net_position(instrument.id) == Decimal(0)
        assert portfolio.net_position(instrument.id, account_id=account.id) == Decimal(0)
        assert portfolio.is_net_flat(instrument.id) is True
        assert portfolio.is_net_flat(instrument.id, account_id=account.id) is True
        assert portfolio.is_completely_net_flat() is True
        assert portfolio.is_completely_net_flat(account_id=account.id) is True
        assert portfolio.is_net_long(instrument.id) is False
        assert portfolio.is_net_short(instrument.id) is False
    finally:
        engine.dispose()


def test_strategy_portfolio_price_overrides_and_currency_conversion_are_fresh():
    usd = Currency.from_str("USD")
    eur = Currency.from_str("EUR")
    jpy = Currency.from_str("JPY")
    venue = Venue("SIM")
    other_venue = Venue("OTHER")
    instrument = TestInstrumentProvider.audusd_sim()
    conversion_instrument = TestInstrumentProvider.default_fx_ccy("EUR/USD")
    PortfolioPositionProbeStrategy.observed_portfolio = None
    PortfolioPositionProbeStrategy.observed_account = None
    PortfolioPositionProbeStrategy.observed_initial_account = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_venue(
        venue=venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(1_000_000.0, usd)],
        base_currency=usd,
    )
    engine.add_venue(
        venue=other_venue,
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        starting_balances=[Money(500_000.0, eur)],
        base_currency=eur,
    )
    engine.add_instrument(instrument)
    engine.add_instrument(conversion_instrument)
    engine.add_data(
        [
            QuoteTick(
                instrument_id=conversion_instrument.id,
                bid_price=Price.from_str("1.25000"),
                ask_price=Price.from_str("1.25000"),
                bid_size=Quantity.from_str("1000000"),
                ask_size=Quantity.from_str("1000000"),
                ts_event=1,
                ts_init=1,
            ),
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str("0.90000"),
                ask_price=Price.from_str("0.90002"),
                bid_size=Quantity.from_str("1000000"),
                ask_size=Quantity.from_str("1000000"),
                ts_event=2,
                ts_init=2,
            ),
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str("1.00000"),
                ask_price=Price.from_str("1.00002"),
                bid_size=Quantity.from_str("1000000"),
                ask_size=Quantity.from_str("1000000"),
                ts_event=3,
                ts_init=3,
            ),
        ],
    )

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:PortfolioPositionProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        portfolio = PortfolioPositionProbeStrategy.observed_portfolio
        account = PortfolioPositionProbeStrategy.observed_account
        initial_account = PortfolioPositionProbeStrategy.observed_initial_account
        assert portfolio is not None
        assert account is not None
        assert initial_account is not None
        assert engine.get_result().total_positions == 1

        instrument_id = instrument.id
        account_id = account.id
        expected_initial_margin = {instrument_id: Money.from_str("0.00 USD")}
        expected_maintenance_margin = {instrument_id: Money.from_str("270.01 USD")}
        assert initial_account.initial_margins() == {}
        assert initial_account.maintenance_margins() == {}
        assert account.initial_margins() == expected_initial_margin
        assert account.maintenance_margins() == expected_maintenance_margin
        assert (
            portfolio.instrument_initial_margins(
                venue=venue,
                account_id=account_id,
            )
            == expected_initial_margin
        )
        assert (
            portfolio.instrument_maintenance_margins(
                venue=venue,
                account_id=account_id,
            )
            == expected_maintenance_margin
        )
        assert portfolio.missing_price_instruments(venue, account_id=account_id) == []
        assert portfolio.net_position(instrument_id, account_id=account_id) == Decimal(100000)
        assert portfolio.is_net_long(instrument_id, account_id=account_id) is True
        assert portfolio.is_net_short(instrument_id, account_id=account_id) is False
        assert portfolio.is_net_flat(instrument_id, account_id=account_id) is False
        assert portfolio.is_completely_net_flat(account_id=account_id) is False

        realized = portfolio.realized_pnl(
            instrument_id,
            account_id=account_id,
            target_currency=eur,
        )
        unrealized = portfolio.unrealized_pnl(
            instrument_id,
            account_id=account_id,
            target_currency=eur,
        )
        total = portfolio.total_pnl(
            instrument_id,
            account_id=account_id,
            target_currency=eur,
        )
        exposure = portfolio.net_exposure(
            instrument_id,
            account_id=account_id,
            target_currency=eur,
        )

        assert realized == Money.from_str("-1.44 EUR")
        assert unrealized == Money.from_str("7998.40 EUR")
        assert total == Money.from_str("7996.96 EUR")
        assert exposure == Money.from_str("80000.00 EUR")
        assert portfolio.realized_pnls(
            venue=venue,
            account_id=account_id,
            target_currency=eur,
        ) == {eur: realized}
        assert portfolio.unrealized_pnls(
            venue=venue,
            account_id=account_id,
            target_currency=eur,
        ) == {eur: unrealized}
        assert portfolio.total_pnls(
            venue=venue,
            account_id=account_id,
            target_currency=eur,
        ) == {eur: total}
        assert portfolio.net_exposures(
            venue=venue,
            account_id=account_id,
            target_currency=eur,
        ) == {eur: exposure}

        cached_unrealized = portfolio.unrealized_pnl(instrument_id, account_id=account_id)
        cached_total = portfolio.total_pnl(instrument_id, account_id=account_id)
        override_price = Price.from_str("1.20000")
        assert portfolio.unrealized_pnl(
            instrument_id,
            price=override_price,
            account_id=account_id,
            target_currency=eur,
        ) == Money.from_str("23998.40 EUR")
        assert portfolio.total_pnl(
            instrument_id,
            price=override_price,
            account_id=account_id,
            target_currency=eur,
        ) == Money.from_str("23996.96 EUR")
        assert portfolio.net_exposure(
            instrument_id,
            price=override_price,
            account_id=account_id,
            target_currency=eur,
        ) == Money.from_str("96000.00 EUR")
        assert portfolio.unrealized_pnl(instrument_id, account_id=account_id) == cached_unrealized
        assert portfolio.total_pnl(instrument_id, account_id=account_id) == cached_total
        assert portfolio.net_exposure(
            instrument_id,
            account_id=account_id,
        ) == Money.from_str("100000.00 USD")
        assert initial_account.initial_margins() == {}
        assert initial_account.maintenance_margins() == {}

        current_account = portfolio.account(account_id=account_id)
        assert current_account.initial_margins() == expected_initial_margin
        assert current_account.maintenance_margins() == expected_maintenance_margin

        assert (
            portfolio.realized_pnl(
                instrument_id,
                account_id=account_id,
                target_currency=jpy,
            )
            is None
        )
        assert (
            portfolio.unrealized_pnl(
                instrument_id,
                account_id=account_id,
                target_currency=jpy,
            )
            is None
        )
        assert (
            portfolio.total_pnl(
                instrument_id,
                account_id=account_id,
                target_currency=jpy,
            )
            is None
        )
        assert (
            portfolio.net_exposure(
                instrument_id,
                account_id=account_id,
                target_currency=jpy,
            )
            is None
        )
        with pytest.raises(RuntimeError, match="failed to calculate realized PnLs"):
            portfolio.realized_pnls(
                venue=venue,
                account_id=account_id,
                target_currency=jpy,
            )
        with pytest.raises(RuntimeError, match="failed to calculate unrealized PnLs"):
            portfolio.unrealized_pnls(
                venue=venue,
                account_id=account_id,
                target_currency=jpy,
            )
        with pytest.raises(RuntimeError, match="failed to calculate total PnLs"):
            portfolio.total_pnls(
                venue=venue,
                account_id=account_id,
                target_currency=jpy,
            )
        assert (
            portfolio.net_exposures(
                venue=venue,
                account_id=account_id,
                target_currency=jpy,
            )
            is None
        )

        other_account_id = AccountId("OTHER-001")
        assert portfolio.realized_pnls(account_id=other_account_id) == {}
        assert portfolio.unrealized_pnls(account_id=other_account_id) == {}
        assert portfolio.total_pnls(account_id=other_account_id) == {}
        assert portfolio.net_exposures(account_id=other_account_id) == {}
    finally:
        engine.dispose()


def test_strategy_portfolio_aggregates_multiple_venues_atomically():
    usd = Currency.from_str("USD")
    eur = Currency.from_str("EUR")
    jpy = Currency.from_str("JPY")
    venues = (Venue("SIM"), Venue("OTHER"), Venue("CLOSED"))
    instruments = (
        TestInstrumentProvider.default_fx_ccy("AUD/USD", venues[0]),
        TestInstrumentProvider.default_fx_ccy("GBP/USD", venues[1]),
        TestInstrumentProvider.default_fx_ccy("NZD/USD", venues[2]),
    )

    eurusd = tuple(TestInstrumentProvider.default_fx_ccy("EUR/USD", venue) for venue in venues)
    usdjpy_sim = TestInstrumentProvider.default_fx_ccy("USD/JPY", venues[0])
    PortfolioMultiVenueProbeStrategy.observed_portfolio = None
    PortfolioMultiVenueProbeStrategy.observed_accounts = None

    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))

    for venue, oms_type in zip(
        venues,
        (OmsType.HEDGING, OmsType.HEDGING, OmsType.NETTING),
        strict=True,
    ):
        engine.add_venue(
            venue=venue,
            oms_type=oms_type,
            account_type=AccountType.MARGIN,
            starting_balances=[Money(1_000_000.0, usd)],
            base_currency=usd,
        )

    for instrument in (*instruments, *eurusd, usdjpy_sim):
        engine.add_instrument(instrument)

    quote_data = (
        (eurusd[0], "1.25000", "1.25000", 1),
        (eurusd[1], "1.25000", "1.25000", 2),
        (eurusd[2], "1.25000", "1.25000", 3),
        (usdjpy_sim, "150.000", "150.000", 4),
        (instruments[0], "0.90000", "0.90002", 10),
        (instruments[1], "1.20000", "1.20002", 11),
        (instruments[2], "0.70000", "0.70002", 12),
        (instruments[0], "1.00000", "1.00002", 20),
        (instruments[1], "1.10000", "1.10002", 21),
        (instruments[2], "0.80000", "0.80002", 22),
    )
    quotes = [
        QuoteTick(
            instrument_id=instrument.id,
            bid_price=Price.from_str(bid),
            ask_price=Price.from_str(ask),
            bid_size=Quantity.from_str("1000000"),
            ask_size=Quantity.from_str("1000000"),
            ts_event=ts_event,
            ts_init=ts_event,
        )
        for instrument, bid, ask, ts_event in quote_data
    ]
    engine.add_data(quotes)

    try:
        engine.add_strategy_from_config(
            ImportableStrategyConfig(
                strategy_path="tests.unit.common.actor:PortfolioMultiVenueProbeStrategy",
                config_path="nautilus_trader.trading:StrategyConfig",
                config={},
            ),
        )
        engine.run()

        portfolio = PortfolioMultiVenueProbeStrategy.observed_portfolio
        accounts = PortfolioMultiVenueProbeStrategy.observed_accounts
        assert portfolio is not None
        assert accounts is not None
        assert engine.get_result().total_positions == 3

        expected_realized = Money.from_str("7992.64 EUR")
        expected_unrealized = Money.from_str("15996.80 EUR")
        expected_total = Money.from_str("23989.44 EUR")
        expected_exposure = Money.from_str("168001.60 EUR")
        assert portfolio.realized_pnls(target_currency=eur) == {eur: expected_realized}
        assert portfolio.unrealized_pnls(target_currency=eur) == {eur: expected_unrealized}
        assert portfolio.total_pnls(target_currency=eur) == {eur: expected_total}
        assert portfolio.net_exposures(target_currency=eur) == {eur: expected_exposure}
        assert portfolio.realized_pnls(venue=venues[2], target_currency=eur) == {
            eur: Money.from_str("7996.00 EUR"),
        }
        assert portfolio.unrealized_pnls(venue=venues[2], target_currency=eur) == {}
        assert portfolio.total_pnls(venue=venues[2], target_currency=eur) == {
            eur: Money.from_str("7996.00 EUR"),
        }

        other_account_id = accounts[venues[1]].id
        assert portfolio.net_position(
            instruments[1].id,
            account_id=other_account_id,
        ) == Decimal(-100000)
        assert portfolio.is_net_long(instruments[1].id, account_id=other_account_id) is False
        assert portfolio.is_net_short(instruments[1].id, account_id=other_account_id) is True
        assert portfolio.is_net_flat(instruments[1].id, account_id=other_account_id) is False
        assert portfolio.is_completely_net_flat(account_id=other_account_id) is False
        assert (
            portfolio.missing_price_instruments(
                venues[1],
                account_id=other_account_id,
            )
            == []
        )

        with pytest.raises(RuntimeError, match="failed to calculate portfolio query"):
            portfolio.realized_pnls(target_currency=jpy)
        with pytest.raises(RuntimeError, match="failed to calculate portfolio query"):
            portfolio.unrealized_pnls(target_currency=jpy)
        with pytest.raises(RuntimeError, match="failed to calculate portfolio query"):
            portfolio.total_pnls(target_currency=jpy)
        assert portfolio.net_exposures(target_currency=jpy) is None
    finally:
        engine.dispose()


LIFECYCLE_METHODS = ["start", "stop", "resume", "reset", "dispose", "degrade", "fault"]
DATA_SUBSCRIPTION_PARAMETERS = ("data_type", "client_id", "params")
DATA_REQUEST_PARAMETERS = ("data_type", "client_id", "start", "end", "limit", "params")
VENUE_SUBSCRIPTION_PARAMETERS = ("venue", "client_id", "params")
VENUE_REQUEST_PARAMETERS = ("venue", "start", "end", "client_id", "params")
INSTRUMENT_SUBSCRIPTION_PARAMETERS = ("instrument_id", "client_id", "params")
BOOK_DELTAS_SUBSCRIPTION_PARAMETERS = (
    "instrument_id",
    "book_type",
    "depth",
    "client_id",
    "managed",
    "params",
)
BOOK_DEPTH10_SUBSCRIPTION_PARAMETERS = (
    "instrument_id",
    "book_type",
    "client_id",
    "managed",
    "params",
)
BOOK_INTERVAL_SUBSCRIPTION_PARAMETERS = (
    "instrument_id",
    "book_type",
    "interval_ms",
    "depth",
    "client_id",
    "params",
)
BOOK_INTERVAL_UNSUBSCRIBE_PARAMETERS = ("instrument_id", "interval_ms", "client_id", "params")
BAR_SUBSCRIPTION_PARAMETERS = ("bar_type", "client_id", "params")
OPTION_CHAIN_SUBSCRIPTION_PARAMETERS = (
    "series_id",
    "strike_range",
    "snapshot_interval_ms",
    "client_id",
    "params",
)
OPTION_CHAIN_UNSUBSCRIBE_PARAMETERS = ("series_id", "client_id")
INSTRUMENT_REQUEST_PARAMETERS = ("instrument_id", "start", "end", "client_id", "params")
BOOK_SNAPSHOT_REQUEST_PARAMETERS = ("instrument_id", "depth", "client_id", "params")
BOOK_DELTAS_REQUEST_PARAMETERS = ("instrument_id", "start", "end", "limit", "client_id", "params")
BOOK_DEPTH_REQUEST_PARAMETERS = (
    "instrument_id",
    "start",
    "end",
    "limit",
    "depth",
    "client_id",
    "params",
)
INSTRUMENT_HISTORY_REQUEST_PARAMETERS = (
    "instrument_id",
    "start",
    "end",
    "limit",
    "client_id",
    "params",
)
BAR_REQUEST_PARAMETERS = ("bar_type", "start", "end", "limit", "client_id", "params")
SUBMIT_ORDER_PARAMETERS = ("order", "position_id", "client_id", "params")
SUBMIT_ORDER_LIST_PARAMETERS = ("order_list", "position_id", "client_id", "params")
MODIFY_ORDER_PARAMETERS = (
    "client_order_id",
    "quantity",
    "price",
    "trigger_price",
    "client_id",
    "params",
)
CANCEL_ORDER_PARAMETERS = ("client_order_id", "client_id", "params")
CANCEL_GTD_EXPIRY_PARAMETERS = ("order",)
CANCEL_ORDERS_PARAMETERS = ("client_order_ids", "client_id", "params")
CANCEL_ALL_ORDERS_PARAMETERS = ("instrument_id", "order_side", "client_id", "params")
CLOSE_POSITION_PARAMETERS = (
    "position",
    "client_id",
    "tags",
    "time_in_force",
    "reduce_only",
    "quote_quantity",
    "params",
)
CLOSE_ALL_POSITIONS_PARAMETERS = (
    "instrument_id",
    "position_side",
    "client_id",
    "tags",
    "time_in_force",
    "reduce_only",
    "quote_quantity",
    "params",
)
QUERY_ACCOUNT_PARAMETERS = ("account_id", "client_id", "params")
QUERY_ORDER_PARAMETERS = ("order", "client_id", "params")
SHUTDOWN_SYSTEM_PARAMETERS = ("reason",)
PUBLISH_DATA_PARAMETERS = ("data_type", "data")
PUBLISH_SIGNAL_PARAMETERS = ("name", "value", "ts_event")
SIGNAL_SUBSCRIPTION_PARAMETERS = ("name", "priority")
SIGNAL_UNSUBSCRIBE_PARAMETERS = ("name",)
STATE_SUBSCRIPTION_PARAMETERS = ("priority",)
STATE_UNSUBSCRIBE_PARAMETERS = ()
SYNTHETIC_PARAMETERS = ("synthetic",)
DATA_SURFACE_SIGNATURES = [
    ("publish_data", PUBLISH_DATA_PARAMETERS),
    ("publish_signal", PUBLISH_SIGNAL_PARAMETERS),
    ("add_synthetic", SYNTHETIC_PARAMETERS),
    ("update_synthetic", SYNTHETIC_PARAMETERS),
    ("subscribe_data", DATA_SUBSCRIPTION_PARAMETERS),
    ("subscribe_signal", SIGNAL_SUBSCRIPTION_PARAMETERS),
    ("subscribe_queue_state", STATE_SUBSCRIPTION_PARAMETERS),
    ("subscribe_socket_state", STATE_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instruments", VENUE_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_book_deltas", BOOK_DELTAS_SUBSCRIPTION_PARAMETERS),
    ("subscribe_book_depth10", BOOK_DEPTH10_SUBSCRIPTION_PARAMETERS),
    ("subscribe_book_at_interval", BOOK_INTERVAL_SUBSCRIPTION_PARAMETERS),
    ("subscribe_quotes", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_trades", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_bars", BAR_SUBSCRIPTION_PARAMETERS),
    ("subscribe_mark_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_index_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_funding_rates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_option_greeks", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument_status", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_instrument_close", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("subscribe_option_chain", OPTION_CHAIN_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_data", DATA_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_signal", SIGNAL_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_queue_state", STATE_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_socket_state", STATE_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_instruments", VENUE_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_book_deltas", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_book_depth10", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_book_at_interval", BOOK_INTERVAL_UNSUBSCRIBE_PARAMETERS),
    ("unsubscribe_quotes", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_trades", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_bars", BAR_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_mark_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_index_prices", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_funding_rates", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_option_greeks", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument_status", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_instrument_close", INSTRUMENT_SUBSCRIPTION_PARAMETERS),
    ("unsubscribe_option_chain", OPTION_CHAIN_UNSUBSCRIBE_PARAMETERS),
    ("request_data", DATA_REQUEST_PARAMETERS),
    ("request_instrument", INSTRUMENT_REQUEST_PARAMETERS),
    ("request_instruments", VENUE_REQUEST_PARAMETERS),
    ("request_book_snapshot", BOOK_SNAPSHOT_REQUEST_PARAMETERS),
    ("request_book_deltas", BOOK_DELTAS_REQUEST_PARAMETERS),
    ("request_book_depth", BOOK_DEPTH_REQUEST_PARAMETERS),
    ("request_quotes", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_trades", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_funding_rates", INSTRUMENT_HISTORY_REQUEST_PARAMETERS),
    ("request_bars", BAR_REQUEST_PARAMETERS),
]
REMOVED_ORDER_EVENT_SUBSCRIPTION_METHODS = [
    "subscribe_order_fills",
    "subscribe_order_cancels",
    "unsubscribe_order_fills",
    "unsubscribe_order_cancels",
]
EXECUTION_SIGNATURES = [
    ("submit_order", SUBMIT_ORDER_PARAMETERS),
    ("submit_order_list", SUBMIT_ORDER_LIST_PARAMETERS),
    ("modify_order", MODIFY_ORDER_PARAMETERS),
    ("cancel_order", CANCEL_ORDER_PARAMETERS),
    ("cancel_gtd_expiry", CANCEL_GTD_EXPIRY_PARAMETERS),
    ("cancel_orders", CANCEL_ORDERS_PARAMETERS),
    ("cancel_all_orders", CANCEL_ALL_ORDERS_PARAMETERS),
    ("close_position", CLOSE_POSITION_PARAMETERS),
    ("close_all_positions", CLOSE_ALL_POSITIONS_PARAMETERS),
    ("query_account", QUERY_ACCOUNT_PARAMETERS),
    ("query_order", QUERY_ORDER_PARAMETERS),
]

NO_PARAMETERS = ()
STATE_PARAMETERS = ("state",)
EVENT_PARAMETERS = ("event",)

LIFECYCLE_HOOK_SIGNATURES = [
    ("on_start", NO_PARAMETERS),
    ("on_stop", NO_PARAMETERS),
    ("on_resume", NO_PARAMETERS),
    ("on_reset", NO_PARAMETERS),
    ("on_dispose", NO_PARAMETERS),
    ("on_degrade", NO_PARAMETERS),
    ("on_fault", NO_PARAMETERS),
]
SAVE_LOAD_HOOK_SIGNATURES = [
    ("on_save", NO_PARAMETERS),
    ("on_load", STATE_PARAMETERS),
]
MARKET_EXIT_HOOK_SIGNATURES = [
    ("on_market_exit", NO_PARAMETERS),
    ("post_market_exit", NO_PARAMETERS),
]
DATA_CALLBACK_SIGNATURES = [
    ("on_time_event", EVENT_PARAMETERS),
    ("on_data", ("data",)),
    ("on_signal", ("signal",)),
    ("on_queue_state", EVENT_PARAMETERS),
    ("on_socket_state", EVENT_PARAMETERS),
    ("on_instrument", ("instrument",)),
    ("on_quote", ("quote",)),
    ("on_trade", ("trade",)),
    ("on_bar", ("bar",)),
    ("on_book_deltas", ("deltas",)),
    ("on_book", ("book",)),
    ("on_mark_price", ("mark_price",)),
    ("on_index_price", ("index_price",)),
    ("on_funding_rate", ("funding_rate",)),
    ("on_instrument_status", ("status",)),
    ("on_instrument_close", ("close",)),
    ("on_option_greeks", ("greeks",)),
    ("on_option_chain", ("slice",)),
]
HISTORICAL_CALLBACK_SIGNATURES = [
    ("on_historical_data", ("data",)),
    ("on_historical_quotes", ("quotes",)),
    ("on_historical_trades", ("trades",)),
    ("on_historical_funding_rates", ("funding_rates",)),
    ("on_historical_bars", ("bars",)),
    ("on_historical_mark_prices", ("mark_prices",)),
    ("on_historical_index_prices", ("index_prices",)),
]
ORDER_CALLBACK_SIGNATURES = [
    ("on_order_initialized", EVENT_PARAMETERS),
    ("on_order_event", EVENT_PARAMETERS),
    ("on_order_denied", EVENT_PARAMETERS),
    ("on_order_emulated", EVENT_PARAMETERS),
    ("on_order_released", EVENT_PARAMETERS),
    ("on_order_submitted", EVENT_PARAMETERS),
    ("on_order_rejected", EVENT_PARAMETERS),
    ("on_order_accepted", EVENT_PARAMETERS),
    ("on_order_expired", EVENT_PARAMETERS),
    ("on_order_triggered", EVENT_PARAMETERS),
    ("on_order_pending_update", EVENT_PARAMETERS),
    ("on_order_pending_cancel", EVENT_PARAMETERS),
    ("on_order_modify_rejected", EVENT_PARAMETERS),
    ("on_order_cancel_rejected", EVENT_PARAMETERS),
    ("on_order_updated", EVENT_PARAMETERS),
    ("on_order_canceled", EVENT_PARAMETERS),
    ("on_order_filled", EVENT_PARAMETERS),
]
POSITION_CALLBACK_SIGNATURES = [
    ("on_position_opened", EVENT_PARAMETERS),
    ("on_position_event", EVENT_PARAMETERS),
    ("on_position_changed", EVENT_PARAMETERS),
    ("on_position_closed", EVENT_PARAMETERS),
]
CALLBACK_SIGNATURES = (
    LIFECYCLE_HOOK_SIGNATURES
    + SAVE_LOAD_HOOK_SIGNATURES
    + MARKET_EXIT_HOOK_SIGNATURES
    + DATA_CALLBACK_SIGNATURES
    + HISTORICAL_CALLBACK_SIGNATURES
    + ORDER_CALLBACK_SIGNATURES
    + POSITION_CALLBACK_SIGNATURES
)


@pytest.mark.parametrize("method_name", LIFECYCLE_METHODS)
def test_strategy_lifecycle_methods_reject_pre_initialized(method_name):
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="Invalid state trigger PRE_INITIALIZED"):
        getattr(strategy, method_name)()


@pytest.mark.parametrize(("method_name", "parameter_names"), EXECUTION_SIGNATURES)
def test_strategy_execution_methods_expose_expected_signatures(method_name, parameter_names):
    strategy = Strategy()
    signature = inspect.signature(getattr(strategy, method_name))

    assert tuple(signature.parameters) == parameter_names


@pytest.mark.parametrize(("method_name", "parameter_names"), DATA_SURFACE_SIGNATURES)
def test_strategy_data_surface_methods_expose_expected_signatures(method_name, parameter_names):
    strategy = Strategy()
    signature = inspect.signature(getattr(strategy, method_name))

    assert tuple(signature.parameters) == parameter_names


@pytest.mark.parametrize("method_name", ["subscribe_queue_state", "subscribe_socket_state"])
def test_strategy_state_subscription_priority_defaults_to_none(method_name):
    signature = inspect.signature(getattr(Strategy(), method_name))

    assert signature.parameters["priority"].default is None


def test_strategy_shutdown_system_exposes_actor_signature():
    strategy = Strategy()
    signature = inspect.signature(strategy.shutdown_system)

    assert tuple(signature.parameters) == SHUTDOWN_SYSTEM_PARAMETERS
    assert signature.parameters["reason"].default is None


def test_strategy_shutdown_system_requires_registration():
    strategy = Strategy()

    with pytest.raises(RuntimeError, match="registered"):
        strategy.shutdown_system("unit test shutdown")


def _subscription_registration_cases():
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")
    bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    strike_range = StrikeRange.atm_relative(1, 1)

    return [
        ("subscribe_data", (DataType("TestData"),)),
        ("subscribe_signal", ("risk",)),
        ("subscribe_queue_state", ()),
        ("subscribe_socket_state", ()),
        ("subscribe_instruments", (Venue("SIM"),)),
        ("subscribe_instrument", (instrument_id,)),
        ("subscribe_book_deltas", (instrument_id, BookType.L2_MBP)),
        ("subscribe_book_depth10", (instrument_id, BookType.L2_MBP)),
        ("subscribe_book_at_interval", (instrument_id, BookType.L2_MBP, 100)),
        ("subscribe_quotes", (instrument_id,)),
        ("subscribe_trades", (instrument_id,)),
        ("subscribe_bars", (bar_type,)),
        ("subscribe_mark_prices", (instrument_id,)),
        ("subscribe_index_prices", (instrument_id,)),
        ("subscribe_funding_rates", (instrument_id,)),
        ("subscribe_option_greeks", (instrument_id,)),
        ("subscribe_instrument_status", (instrument_id,)),
        ("subscribe_instrument_close", (instrument_id,)),
        ("subscribe_option_chain", (series_id, strike_range)),
        ("unsubscribe_data", (DataType("TestData"),)),
        ("unsubscribe_signal", ("risk",)),
        ("unsubscribe_queue_state", ()),
        ("unsubscribe_socket_state", ()),
        ("unsubscribe_instruments", (Venue("SIM"),)),
        ("unsubscribe_instrument", (instrument_id,)),
        ("unsubscribe_book_deltas", (instrument_id,)),
        ("unsubscribe_book_depth10", (instrument_id,)),
        ("unsubscribe_book_at_interval", (instrument_id, 100)),
        ("unsubscribe_quotes", (instrument_id,)),
        ("unsubscribe_trades", (instrument_id,)),
        ("unsubscribe_bars", (bar_type,)),
        ("unsubscribe_mark_prices", (instrument_id,)),
        ("unsubscribe_index_prices", (instrument_id,)),
        ("unsubscribe_funding_rates", (instrument_id,)),
        ("unsubscribe_option_greeks", (instrument_id,)),
        ("unsubscribe_instrument_status", (instrument_id,)),
        ("unsubscribe_instrument_close", (instrument_id,)),
        ("unsubscribe_option_chain", (series_id,)),
    ]


def _data_operation_registration_cases():
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")
    bar_type = BarType.from_str("AUD/USD.SIM-1-MINUTE-LAST-EXTERNAL")
    custom_data = _model_custom_data()
    synthetic = _synthetic("(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2")

    return [
        ("publish_data", (custom_data.data_type, custom_data)),
        ("publish_signal", ("risk", "value")),
        ("add_synthetic", (synthetic,)),
        ("update_synthetic", (synthetic,)),
        ("request_data", (DataType("TestData"), ClientId("SIM"))),
        ("request_instrument", (instrument_id,)),
        ("request_instruments", (Venue("SIM"),)),
        ("request_book_snapshot", (instrument_id,)),
        ("request_book_deltas", (instrument_id,)),
        ("request_book_depth", (instrument_id,)),
        ("request_quotes", (instrument_id,)),
        ("request_trades", (instrument_id,)),
        ("request_funding_rates", (instrument_id,)),
        ("request_bars", (bar_type,)),
    ]


def _model_custom_data():
    class Payload:
        ts_event = 3
        ts_init = 4

    return nautilus_trader.model.CustomData(DataType("Payload"), Payload())


def _synthetic(formula):
    return SyntheticInstrument(
        symbol=Symbol("BTC-ETH"),
        price_precision=8,
        components=[
            TestInstrumentProvider.btcusdt_binance().id,
            TestInstrumentProvider.ethusdt_binance().id,
        ],
        formula=formula,
        ts_event=0,
        ts_init=1,
    )


@pytest.mark.parametrize(("method_name", "args"), _subscription_registration_cases())
def test_strategy_subscriptions_require_registration(method_name, args):
    strategy = Strategy()

    with pytest.raises(RuntimeError) as exc_info:
        getattr(strategy, method_name)(*args)

    assert str(exc_info.value) == "Strategy must be registered before managing subscriptions"


@pytest.mark.parametrize(("method_name", "args"), _data_operation_registration_cases())
def test_strategy_data_operations_require_registration(method_name, args):
    strategy = Strategy()

    with pytest.raises(RuntimeError) as exc_info:
        getattr(strategy, method_name)(*args)

    assert str(exc_info.value) == DATA_OPERATION_REGISTRATION_ERROR


def test_strategy_registration_precedes_publish_signal_conversion():
    class InvalidSignalValue:
        def __str__(self):
            raise ValueError("invalid signal value")

    with pytest.raises(RuntimeError) as exc_info:
        Strategy().publish_signal("risk", InvalidSignalValue())

    assert str(exc_info.value) == DATA_OPERATION_REGISTRATION_ERROR


def test_strategy_registration_precedes_request_params_conversion():
    with pytest.raises(RuntimeError) as exc_info:
        Strategy().request_instruments(params={"invalid": object()})

    assert str(exc_info.value) == DATA_OPERATION_REGISTRATION_ERROR


def test_strategy_data_operations_succeed_when_registered():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    strategy = Strategy()
    custom_data = _model_custom_data()
    synthetic = _synthetic("(BTCUSDT.BINANCE + ETHUSDT.BINANCE) / 2")
    updated = _synthetic("BTCUSDT.BINANCE + ETHUSDT.BINANCE")
    engine.add_strategy(strategy)

    try:
        assert strategy.publish_data(custom_data.data_type, custom_data) is None
        assert strategy.publish_signal("risk", "value") is None
        assert strategy.add_synthetic(synthetic) is None
        assert strategy.update_synthetic(updated) is None
        assert strategy.cache.synthetic(updated.id) == updated
    finally:
        engine.dispose()


def test_strategy_subscription_validation_precedes_registration():
    strategy = Strategy()
    instrument_id = InstrumentId.from_str("AUD/USD.SIM")

    with pytest.raises(ValueError, match="interval_ms must be > 0"):
        strategy.subscribe_book_at_interval(
            instrument_id,
            BookType.L2_MBP,
            0,
            params={"invalid": object()},
        )


def test_strategy_registration_precedes_params_conversion():
    strategy = Strategy()

    with pytest.raises(RuntimeError) as exc_info:
        strategy.subscribe_data(DataType("TestData"), params={"invalid": object()})

    assert str(exc_info.value) == "Strategy must be registered before managing subscriptions"


@pytest.mark.parametrize("method_name", REMOVED_ORDER_EVENT_SUBSCRIPTION_METHODS)
def test_strategy_order_event_subscription_methods_are_not_exposed(method_name):
    strategy = Strategy()

    assert not hasattr(strategy, method_name)


@pytest.mark.parametrize(("method_name", "parameter_names"), CALLBACK_SIGNATURES)
def test_strategy_callback_methods_expose_expected_signatures(method_name, parameter_names):
    strategy = Strategy()
    signature = inspect.signature(getattr(strategy, method_name))

    assert tuple(signature.parameters) == parameter_names


@pytest.mark.parametrize("request_time", HISTORICAL_REQUEST_DATETIME_CASES)
def test_strategy_historical_requests_accept_datetimes_when_registered(request_time):
    HistoricalRequestProbeStrategy.observed_request_ids = {}
    HistoricalRequestProbeStrategy.request_time = _historical_request_time(request_time)
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True, run_analysis=False))
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="tests.unit.trading.test_trading:HistoricalRequestProbeStrategy",
            config_path="nautilus_trader.trading:StrategyConfig",
            config={},
        ),
    )

    try:
        engine.run()

        assert set(HistoricalRequestProbeStrategy.observed_request_ids) == {
            "data",
            "instrument",
            "instruments",
            "book_snapshot",
            "book_deltas",
            "book_depth",
            "quotes",
            "trades",
            "funding_rates",
            "bars",
        }

        for request_id in HistoricalRequestProbeStrategy.observed_request_ids.values():
            assert UUID4.from_str(request_id)
    finally:
        engine.dispose()


def _historical_request_time(request_time):
    if request_time == "datetime-utc":
        return dt.datetime(1970, 1, 1, tzinfo=dt.UTC)

    pd = pytest.importorskip("pandas")

    if request_time == "pandas-timestamp-utc":
        return pd.Timestamp("1970-01-01T00:00:00Z")

    if request_time == "pandas-timestamp-utc-nanos":
        return pd.Timestamp(0, unit="ns", tz="UTC")

    raise ValueError(f"Unknown historical request datetime case: {request_time}")


def test_strategy_config_defaults():
    config = StrategyConfig()

    assert config.strategy_id is None
    assert config.order_id_tag is None
    assert config.oms_type is None
    assert config.external_order_claims is None
    assert config.manage_contingent_orders is False
    assert config.manage_gtd_expiry is False
    assert config.manage_stop is False
    assert config.market_exit_interval_ms == 100
    assert config.market_exit_max_attempts == 100
    assert config.market_exit_time_in_force == TimeInForce.GTC
    assert config.market_exit_reduce_only is True
    assert config.use_uuid_client_order_ids is False
    assert config.use_hyphens_in_client_order_ids is True
    assert config.log_events is True
    assert config.log_commands is True
    assert config.log_rejected_due_post_only_as_warning is True


def test_strategy_config_with_explicit_values():
    external_order_claims = [InstrumentId.from_str("ETH/USDT.BINANCE")]
    config = StrategyConfig(
        StrategyId("S-002"),
        "002",
        OmsType.HEDGING,
        external_order_claims,
        True,
        True,
        True,
        500,
        5,
        TimeInForce.IOC,
        False,
        False,
        False,
        False,
        False,
        True,
    )

    assert config.strategy_id == StrategyId("S-002")
    assert config.order_id_tag == "002"
    assert config.oms_type == OmsType.HEDGING
    assert config.external_order_claims == external_order_claims
    assert config.manage_contingent_orders is True
    assert config.manage_gtd_expiry is True
    assert config.manage_stop is True
    assert config.market_exit_interval_ms == 500
    assert config.market_exit_max_attempts == 5
    assert config.market_exit_time_in_force == TimeInForce.IOC
    assert config.market_exit_reduce_only is False
    assert config.use_uuid_client_order_ids is False
    assert config.use_hyphens_in_client_order_ids is False
    assert config.log_events is False
    assert config.log_commands is False
    assert config.log_rejected_due_post_only_as_warning is True


def test_forex_session_variants():
    variants = list(ForexSession.variants())

    assert len(variants) == 4
    assert ForexSession.from_str("SYDNEY") == ForexSession.SYDNEY
    assert ForexSession.from_str("TOKYO") == ForexSession.TOKYO
    assert ForexSession.from_str("LONDON") == ForexSession.LONDON
    assert ForexSession.from_str("NEW_YORK") == ForexSession.NEW_YORK


NOW_UTC = dt.datetime(2024, 6, 15, 12, 0, 0, tzinfo=dt.UTC)


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_start_returns_future_datetime(session):
    result = fx_next_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_next_end_returns_future_datetime(session):
    result = fx_next_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result > NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_start_returns_past_datetime(session):
    result = fx_prev_start(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_prev_end_returns_past_datetime(session):
    result = fx_prev_end(session, NOW_UTC)

    assert isinstance(result, dt.datetime)
    assert result < NOW_UTC


@pytest.mark.parametrize("session", list(ForexSession.variants()))
def test_fx_local_from_utc_returns_string(session):
    result = fx_local_from_utc(session, NOW_UTC)

    assert isinstance(result, str)
    assert "2024" in result


HOOK_METHODS = [
    "on_start",
    "on_stop",
    "on_resume",
    "on_reset",
    "on_dispose",
    "on_degrade",
    "on_fault",
]

DATA_CALLBACKS = [
    ("on_time_event", "time_event"),
    ("on_data", "custom_data"),
    ("on_signal", "signal"),
    ("on_queue_state", "queue_state_changed"),
    ("on_socket_state", "socket_state_changed"),
    ("on_instrument", "instrument"),
    ("on_quote", "quote"),
    ("on_trade", "trade"),
    ("on_bar", "bar"),
    ("on_book_deltas", "book_deltas"),
    ("on_book", "book"),
    ("on_mark_price", "mark_price"),
    ("on_index_price", "index_price"),
    ("on_funding_rate", "funding_rate"),
    ("on_instrument_status", "instrument_status"),
    ("on_instrument_close", "instrument_close"),
    ("on_option_greeks", "option_greeks"),
    ("on_option_chain", "option_chain"),
    ("on_historical_data", "historical_data"),
    ("on_historical_quotes", "historical_quotes"),
    ("on_historical_trades", "historical_trades"),
    ("on_historical_funding_rates", "historical_funding_rates"),
    ("on_historical_bars", "historical_bars"),
    ("on_historical_mark_prices", "historical_mark_prices"),
    ("on_historical_index_prices", "historical_index_prices"),
]

ORDER_CALLBACKS = [
    ("on_order_initialized", "order_initialized"),
    ("on_order_denied", "order_denied"),
    ("on_order_emulated", "order_emulated"),
    ("on_order_released", "order_released"),
    ("on_order_submitted", "order_submitted"),
    ("on_order_rejected", "order_rejected"),
    ("on_order_accepted", "order_accepted"),
    ("on_order_expired", "order_expired"),
    ("on_order_triggered", "order_triggered"),
    ("on_order_pending_update", "order_pending_update"),
    ("on_order_pending_cancel", "order_pending_cancel"),
    ("on_order_modify_rejected", "order_modify_rejected"),
    ("on_order_cancel_rejected", "order_cancel_rejected"),
    ("on_order_updated", "order_updated"),
    ("on_order_canceled", "order_canceled"),
    ("on_order_filled", "order_filled"),
]

POSITION_CALLBACKS = [
    ("on_position_opened", "position_opened"),
    ("on_position_changed", "position_changed"),
    ("on_position_closed", "position_closed"),
]


def _make_recording_method(method_name):
    def method(self, *args):
        self.calls.append((method_name, args))

    return method


def _create_recording_strategy_type():
    attrs = {}

    for method_name in HOOK_METHODS:
        attrs[method_name] = _make_recording_method(method_name)

    for method_name, _sample_name in DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS:
        attrs[method_name] = _make_recording_method(method_name)

    return type("RecordingStrategy", (TestStrategy,), attrs)


RecordingStrategy = _create_recording_strategy_type()


@pytest.fixture
def strategy():
    return Strategy()


@pytest.fixture
def recording_strategy():
    strategy = RecordingStrategy()
    strategy.calls = []
    return strategy


@pytest.fixture
def strategy_sample_objects():
    instrument = TestInstrumentProvider.audusd_sim()
    quote = _make_quote(instrument.id)
    trade = _make_trade(instrument.id)
    bar = _make_bar(instrument.id)
    book_deltas = _make_book_deltas(instrument.id)
    option_greeks = _make_option_greeks()
    option_chain = _make_option_chain()
    time_event = TimeEvent("timer", UUID4(), 1, 2)
    custom_data = CustomData(DataType("X"), [1, 2], 3, 4)
    signal = Signal("sig", "value", 1, 2)
    queue_state_changed = QueueStateChanged(
        TraderId("TRADER-001"),
        SystemChannel.EXEC_COMMANDS,
        QueueCondition.BACKLOGGED,
        QueueState.TRIGGERED,
        17,
        23,
        UUID4(),
        7,
        8,
    )
    socket_state_changed = SocketStateChanged(
        TraderId("TRADER-001"),
        ClientId("BINANCE"),
        Venue("BINANCE"),
        "binance-futures-market-streams",
        SocketState.CONNECTED,
        UUID4(),
        7,
        8,
    )
    mark_price = MarkPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    index_price = IndexPriceUpdate(instrument.id, Price.from_str("1.00000"), 1, 2)
    funding_rate = FundingRateUpdate(instrument.id, Decimal("0.0001"), 1, 2, interval=480)
    instrument_status = InstrumentStatus(instrument.id, MarketStatusAction.TRADING, 1, 2)
    instrument_close = InstrumentClose(
        instrument.id,
        Price.from_str("1.00000"),
        InstrumentCloseType.END_OF_SESSION,
        1,
        2,
    )
    order_events = _make_order_events(instrument)
    position_events = _make_position_events(instrument)

    return {
        "time_event": time_event,
        "custom_data": custom_data,
        "signal": signal,
        "queue_state_changed": queue_state_changed,
        "socket_state_changed": socket_state_changed,
        "instrument": instrument,
        "quote": quote,
        "trade": trade,
        "bar": bar,
        "book_deltas": book_deltas,
        "book": OrderBook(instrument.id, BookType.L2_MBP),
        "mark_price": mark_price,
        "index_price": index_price,
        "funding_rate": funding_rate,
        "instrument_status": instrument_status,
        "instrument_close": instrument_close,
        "option_greeks": option_greeks,
        "option_chain": option_chain,
        "historical_data": custom_data,
        "historical_quotes": [quote],
        "historical_trades": [trade],
        "historical_funding_rates": [funding_rate],
        "historical_bars": [bar],
        "historical_mark_prices": [mark_price],
        "historical_index_prices": [index_price],
        **order_events,
        **position_events,
    }


@pytest.mark.parametrize("method_name", HOOK_METHODS)
def test_strategy_lifecycle_hooks_can_be_overridden(recording_strategy, method_name):
    assert getattr(recording_strategy, method_name)() is None

    assert recording_strategy.calls[-1] == (method_name, ())


@pytest.mark.parametrize(
    ("method_name", "sample_name"),
    DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS,
)
def test_strategy_callbacks_accept_runtime_objects(
    strategy,
    strategy_sample_objects,
    method_name,
    sample_name,
):
    assert getattr(strategy, method_name)(strategy_sample_objects[sample_name]) is None


@pytest.mark.parametrize(
    ("method_name", "sample_name"),
    DATA_CALLBACKS + ORDER_CALLBACKS + POSITION_CALLBACKS,
)
def test_strategy_overridden_callbacks_receive_runtime_objects(
    recording_strategy,
    strategy_sample_objects,
    method_name,
    sample_name,
):
    payload = strategy_sample_objects[sample_name]

    assert getattr(recording_strategy, method_name)(payload) is None

    call_name, call_args = recording_strategy.calls[-1]
    assert call_name == method_name
    assert call_args == (payload,)
    assert call_args[0] is payload


def _make_quote(instrument_id):
    return QuoteTick(
        instrument_id,
        Price.from_str("1.00000"),
        Price.from_str("1.00001"),
        Quantity.from_int(1),
        Quantity.from_int(2),
        1,
        2,
    )


def _make_trade(instrument_id):
    return TradeTick(
        instrument_id,
        Price.from_str("1.00000"),
        Quantity.from_int(10),
        AggressorSide.BUYER,
        TradeId("T-001"),
        1,
        2,
    )


def _make_bar(instrument_id):
    bar_type = BarType.from_str(f"{instrument_id}-1-MINUTE-LAST-EXTERNAL")
    return Bar(
        bar_type,
        Price.from_str("1.00000"),
        Price.from_str("1.10000"),
        Price.from_str("0.90000"),
        Price.from_str("1.05000"),
        Quantity.from_int(100),
        1,
        2,
    )


def _make_book_deltas(instrument_id):
    bid = BookOrder(OrderSide.BUY, Price.from_str("1.00000"), Quantity.from_int(1), 1)
    ask = BookOrder(OrderSide.SELL, Price.from_str("1.10000"), Quantity.from_int(2), 2)
    delta1 = OrderBookDelta(instrument_id, BookAction.ADD, bid, 0, 1, 1, 2)
    delta2 = OrderBookDelta(instrument_id, BookAction.ADD, ask, 0, 2, 1, 2)
    return OrderBookDeltas(instrument_id, [delta1, delta2])


def _make_option_greeks():
    instrument_id = InstrumentId.from_str("BTC-20240329-50000-C.DERIBIT")
    return OptionGreeks(
        instrument_id,
        0.5,
        0.1,
        0.2,
        -0.3,
        0.05,
        0.6,
        0.55,
        0.65,
        50_000.0,
        42.0,
        3,
        4,
    )


def _make_option_chain():
    series_id = OptionSeriesId.from_expiry("DERIBIT", "BTC", "USD", "2024-03-29")
    return OptionChainSlice(series_id, Price.from_str("50000.0"), 5, 6)


def _make_order_events(instrument):
    trader_id = TraderId("TRADER-001")
    strategy_id = StrategyId("S-001")
    account_id = AccountId("SIM-001")
    client_order_id = ClientOrderId("O-001")
    venue_order_id = VenueOrderId("V-001")

    return {
        "order_initialized": OrderInitialized(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            order_side=OrderSide.BUY,
            order_type=OrderType.MARKET,
            quantity=Quantity.from_int(100_000),
            time_in_force=TimeInForce.GTC,
            post_only=False,
            reduce_only=False,
            quote_quantity=False,
            reconciliation=False,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_denied": OrderDenied(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="denied",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_emulated": OrderEmulated(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_released": OrderReleased(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            released_price=Price.from_str("1.00020"),
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_submitted": OrderSubmitted(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
        ),
        "order_rejected": OrderRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            reason="rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
        ),
        "order_accepted": OrderAccepted(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            venue_order_id=venue_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
        ),
        "order_expired": OrderExpired(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_triggered": OrderTriggered(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_pending_update": OrderPendingUpdate(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
        ),
        "order_pending_cancel": OrderPendingCancel(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            account_id=account_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
        ),
        "order_modify_rejected": OrderModifyRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="modify rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_cancel_rejected": OrderCancelRejected(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            reason="cancel rejected",
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_updated": OrderUpdated(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            quantity=Quantity.from_int(150_000),
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
            price=Price.from_str("1.00030"),
        ),
        "order_canceled": OrderCanceled(
            trader_id=trader_id,
            strategy_id=strategy_id,
            instrument_id=instrument.id,
            client_order_id=client_order_id,
            event_id=UUID4(),
            ts_event=1,
            ts_init=2,
            reconciliation=False,
            venue_order_id=venue_order_id,
            account_id=account_id,
        ),
        "order_filled": _make_order_filled_event(
            instrument,
            client_order_id="O-001",
            venue_order_id="V-001",
            trade_id="T-001",
            order_side=OrderSide.BUY,
            last_qty=100_000,
            last_px="1.00010",
            ts_event=10,
        ),
    }


def _make_position_events(instrument):
    position_id = "P-101"
    opening_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-101",
        venue_order_id="V-101",
        trade_id="T-101",
        order_side=OrderSide.BUY,
        last_qty=100_000,
        last_px="1.00010",
        position_id=position_id,
        ts_event=10,
    )
    position = Position(instrument=instrument, fill=opening_fill)
    position_opened = PositionOpened.create(position, opening_fill, UUID4(), 11)

    change_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-102",
        venue_order_id="V-102",
        trade_id="T-102",
        order_side=OrderSide.BUY,
        last_qty=50_000,
        last_px="1.00020",
        position_id=position_id,
        ts_event=12,
    )
    position.apply(change_fill)
    position_changed = PositionChanged.create(position, change_fill, UUID4(), 13)

    closing_fill = _make_order_filled_event(
        instrument,
        client_order_id="O-103",
        venue_order_id="V-103",
        trade_id="T-103",
        order_side=OrderSide.SELL,
        last_qty=150_000,
        last_px="1.00030",
        position_id=position_id,
        ts_event=14,
    )
    position.apply(closing_fill)
    position_closed = PositionClosed.create(position, closing_fill, UUID4(), 15)

    return {
        "position_opened": position_opened,
        "position_changed": position_changed,
        "position_closed": position_closed,
    }


def test_position_change_and_close_events_expose_peak_qty():
    events = _make_position_events(TestInstrumentProvider.audusd_sim())

    assert events["position_changed"].peak_qty == Quantity.from_int(150_000)
    assert events["position_closed"].peak_qty == Quantity.from_int(150_000)


def _make_order_filled_event(
    instrument,
    client_order_id,
    venue_order_id,
    trade_id,
    order_side,
    last_qty,
    last_px,
    ts_event,
    position_id=None,
):
    return OrderFilled(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=instrument.id,
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId(venue_order_id),
        account_id=AccountId("SIM-001"),
        trade_id=TradeId(trade_id),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(last_qty),
        last_px=Price.from_str(last_px),
        currency=instrument.quote_currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=ts_event,
        ts_init=ts_event + 1,
        reconciliation=False,
        position_id=PositionId(position_id) if position_id is not None else None,
        commission=Money.from_str("2.00 USD"),
    )
