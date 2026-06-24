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

import math

import pytest

from nautilus_trader.analysis import ReportProvider
from nautilus_trader.backtest import BacktestEngine
from nautilus_trader.backtest import BacktestEngineConfig
from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import AccountType
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import Currency
from nautilus_trader.model import LiquiditySide
from nautilus_trader.model import MarketOrder
from nautilus_trader.model import Money
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderFilled
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderSubmitted
from nautilus_trader.model import OrderType
from nautilus_trader.model import Position
from nautilus_trader.model import PositionId
from nautilus_trader.model import Price
from nautilus_trader.model import Quantity
from nautilus_trader.model import QuoteTick
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TimeInForce
from nautilus_trader.model import TradeId
from nautilus_trader.model import TraderId
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId
from nautilus_trader.trading import ImportableStrategyConfig
from tests.providers import TestInstrumentProvider


pd = pytest.importorskip("pandas")

AUDUSD = TestInstrumentProvider.audusd_sim()
TRADER_ID = TraderId("TESTER-001")
STRATEGY_ID = StrategyId("S-001")
ACCOUNT_ID = AccountId("SIM-000")


def _make_fill(
    client_order_id="O-001",
    position_id="P-001",
    order_side=OrderSide.BUY,
    last_px="1.00001",
    last_qty=100_000,
    ts_event=1_000_000_000,
):
    return OrderFilled(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD.id,
        client_order_id=ClientOrderId(client_order_id),
        venue_order_id=VenueOrderId("1"),
        account_id=ACCOUNT_ID,
        trade_id=TradeId(f"E-{client_order_id}"),
        order_side=order_side,
        order_type=OrderType.MARKET,
        last_qty=Quantity.from_int(last_qty),
        last_px=Price.from_str(last_px),
        currency=AUDUSD.quote_currency,
        liquidity_side=LiquiditySide.TAKER,
        event_id=UUID4(),
        ts_event=ts_event,
        ts_init=ts_event,
        reconciliation=False,
        position_id=PositionId(position_id),
        commission=Money.from_str("2.00 USD"),
    )


def _make_filled_order(client_order_id="O-001", position_id="P-001"):
    order = MarketOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD.id,
        client_order_id=ClientOrderId(client_order_id),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(100_000),
        time_in_force=TimeInForce.GTC,
        init_id=UUID4(),
        ts_init=0,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )
    submitted = OrderSubmitted(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD.id,
        client_order_id=ClientOrderId(client_order_id),
        account_id=ACCOUNT_ID,
        event_id=UUID4(),
        ts_event=500_000_000,
        ts_init=500_000_000,
    )
    order.apply(submitted)
    fill = _make_fill(client_order_id=client_order_id, position_id=position_id)
    order.apply(fill)
    return order


def _make_position(client_order_id="O-001", position_id="P-001"):
    fill = _make_fill(client_order_id=client_order_id, position_id=position_id)
    return Position(instrument=AUDUSD, fill=fill)


def _engine_with_account():
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.HEDGING,
        account_type=AccountType.MARGIN,
        base_currency=Currency.from_str("USD"),
        starting_balances=[Money(1_000_000.0, Currency.from_str("USD"))],
    )
    return engine


# Empty-input cases


def test_generate_orders_report_empty():
    report = ReportProvider.generate_orders_report([])

    assert isinstance(report, pd.DataFrame)
    assert report.empty


def test_generate_order_fills_report_empty():
    report = ReportProvider.generate_order_fills_report([])

    assert isinstance(report, pd.DataFrame)
    assert report.empty


def test_generate_fills_report_empty():
    report = ReportProvider.generate_fills_report([])

    assert isinstance(report, pd.DataFrame)
    assert report.empty


def test_generate_positions_report_empty():
    report = ReportProvider.generate_positions_report([])

    assert isinstance(report, pd.DataFrame)
    assert report.empty


# Non-empty structure cases


def test_generate_orders_report_index():
    order = _make_filled_order()

    report = ReportProvider.generate_orders_report([order])

    assert report.index.name == "client_order_id"
    assert "O-001" in report.index


def test_generate_order_fills_report_excludes_unfilled():
    filled = _make_filled_order("O-001")
    unfilled = MarketOrder(
        trader_id=TRADER_ID,
        strategy_id=STRATEGY_ID,
        instrument_id=AUDUSD.id,
        client_order_id=ClientOrderId("O-002"),
        order_side=OrderSide.BUY,
        quantity=Quantity.from_int(50_000),
        time_in_force=TimeInForce.GTC,
        init_id=UUID4(),
        ts_init=0,
        reduce_only=False,
        quote_quantity=False,
        contingency_type=ContingencyType.NO_CONTINGENCY,
    )

    report = ReportProvider.generate_order_fills_report([filled, unfilled])

    assert report.index.name == "client_order_id"
    assert "O-001" in report.index
    assert "O-002" not in report.index
    assert isinstance(report["ts_last"].iloc[0], pd.Timestamp)


def test_generate_fills_report_structure():
    order = _make_filled_order()

    report = ReportProvider.generate_fills_report([order])

    assert report.index.name == "client_order_id"
    assert isinstance(report["ts_event"].iloc[0], pd.Timestamp)
    assert "type" not in report.columns


def test_generate_positions_report_structure():
    position = _make_position()

    report = ReportProvider.generate_positions_report([position])

    assert report.index.name == "position_id"
    assert "is_snapshot" in report.columns
    assert "signed_qty" not in report.columns
    assert "quote_currency" not in report.columns
    assert "base_currency" not in report.columns
    assert "settlement_currency" not in report.columns


def test_generate_positions_report_snapshot_flag():
    position = _make_position("O-001", "P-001")
    snapshot = _make_position("O-002", "P-002")

    report = ReportProvider.generate_positions_report([position], snapshots=[snapshot])

    assert not report.loc["P-001", "is_snapshot"]
    assert report.loc["P-002", "is_snapshot"]


def test_generate_account_report_structure():
    engine = _engine_with_account()
    engine.run()
    account = engine.cache.account_for_venue(Venue("SIM"))

    report = ReportProvider.generate_account_report(account)
    engine.dispose()

    assert isinstance(report, pd.DataFrame)
    assert not report.empty
    assert "ts_init" not in report.columns
    assert "type" not in report.columns
    assert "event_id" not in report.columns
    assert isinstance(report.index[0], pd.Timestamp)


# End-to-end parity test: real backtest with fills and closed positions


_E2E_TS_START = 1_577_836_800_000_000_000
_E2E_BID_PRICES = ("0.70000", "0.70000", "0.70010", "0.70020", "0.70020")


def _e2e_quotes(instrument) -> list[QuoteTick]:
    quotes: list[QuoteTick] = []

    for idx, bid_price in enumerate(_E2E_BID_PRICES):
        ts = _E2E_TS_START + idx * 60_000_000_000
        quotes.append(
            QuoteTick(
                instrument_id=instrument.id,
                bid_price=Price.from_str(bid_price),
                ask_price=Price.from_str(bid_price),
                bid_size=Quantity.from_int(1_000_000),
                ask_size=Quantity.from_int(1_000_000),
                ts_event=ts,
                ts_init=ts,
            ),
        )
    return quotes


def _run_small_backtest_with_fills() -> BacktestEngine:
    audusd = TestInstrumentProvider.audusd_sim()
    usd = Currency.from_str("USD")
    engine = BacktestEngine(BacktestEngineConfig(bypass_logging=True))
    engine.add_venue(
        venue=Venue("SIM"),
        oms_type=OmsType.NETTING,
        account_type=AccountType.MARGIN,
        base_currency=usd,
        starting_balances=[Money(1_000_000.0, usd)],
    )
    engine.add_instrument(audusd)
    engine.add_data(_e2e_quotes(audusd))
    engine.add_strategy_from_config(
        ImportableStrategyConfig(
            strategy_path="strategies.acceptance:TickScheduled",
            config_path="strategies.acceptance:TickScheduledConfig",
            config={
                "instrument_id": str(audusd.id),
                "actions": [(2, "BUY", "100000"), (4, "SELL", "100000")],
            },
        ),
    )
    engine.run()
    return engine


def _float_maps_equal(a: dict, b: dict) -> bool:
    if a.keys() != b.keys():
        return False
    for key in a:
        va, vb = a[key], b[key]
        if math.isnan(va) and math.isnan(vb):
            continue
        if va != vb:
            return False
    return True


def _stats_equal(a: dict, b: dict) -> bool:
    if a.keys() != b.keys():
        return False
    for key in a:
        va, vb = a[key], b[key]
        if isinstance(va, dict) and isinstance(vb, dict):
            if not _float_maps_equal(va, vb):
                return False
        elif math.isnan(va) and math.isnan(vb):
            continue
        elif va != vb:
            return False
    return True


def test_end_to_end_reporting_and_statistics():
    engine = _run_small_backtest_with_fills()

    orders = ReportProvider.generate_orders_report(engine.cache.orders())
    fills = ReportProvider.generate_fills_report(engine.cache.orders())
    positions = ReportProvider.generate_positions_report(
        engine.cache.positions(),
        engine.cache.position_snapshots(),
    )
    assert not orders.empty
    assert orders.index.name == "client_order_id"
    assert not fills.empty
    assert not positions.empty
    assert "is_snapshot" in positions.columns

    stats = engine.portfolio.statistics()
    result = engine.get_result()
    assert stats.pnls, "stats.pnls should be non-empty for a run with closed positions"
    assert _stats_equal(stats.pnls, result.stats_pnls)
    assert _stats_equal(stats.returns, result.stats_returns)
    assert _stats_equal(stats.general, result.stats_general)

    engine.dispose()


def test_engine_generate_reports_match_reportprovider():
    engine = _run_small_backtest_with_fills()

    orders = engine.generate_orders_report()
    assert not orders.empty
    assert orders.index.name == "client_order_id"
    expected = ReportProvider.generate_orders_report(engine.cache.orders())
    assert orders.equals(expected)

    positions = engine.generate_positions_report()
    assert "is_snapshot" in positions.columns

    venue = engine.list_venues()[0]
    account = engine.generate_account_report(venue=venue)
    assert isinstance(account, pd.DataFrame)

    engine.dispose()
