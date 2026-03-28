from __future__ import annotations

from decimal import Decimal
import importlib
from pathlib import Path

import pytest

from nautilus_trader.common.component import LiveClock
from nautilus_trader.common.component import MessageBus
from nautilus_trader.common.providers import InstrumentProvider
from nautilus_trader.core.uuid import UUID4
from nautilus_trader.execution.messages import GenerateFillReports
from nautilus_trader.execution.messages import GenerateOrderStatusReport
from nautilus_trader.execution.messages import GenerateOrderStatusReports
from nautilus_trader.execution.reports import FillReport
from nautilus_trader.execution.reports import OrderStatusReport
from nautilus_trader.execution.reports import PositionStatusReport
from nautilus_trader.model.currencies import USD
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.enums import LiquiditySide
from nautilus_trader.model.enums import OmsType
from nautilus_trader.model.enums import OrderSide
from nautilus_trader.model.enums import OrderStatus
from nautilus_trader.model.enums import OrderType
from nautilus_trader.model.enums import PositionSide
from nautilus_trader.model.enums import TimeInForce
from nautilus_trader.model.identifiers import ClientId
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import StrategyId
from nautilus_trader.model.identifiers import TradeId
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.identifiers import VenueOrderId
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.mocks.exec_clients import MockLiveExecutionClient
from nautilus_trader.test_kit.providers import TestInstrumentProvider
from nautilus_trader.test_kit.stubs.component import TestComponentStubs
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


SIM = Venue("SIM")
AUDUSD_SIM = TestInstrumentProvider.default_fx_ccy("AUD/USD")
GBPUSD_SIM = TestInstrumentProvider.default_fx_ccy("GBP/USD")


def _repo_root() -> Path:
    return Path(__file__).resolve().parents[4]


def _load_nautilus_adapter_module():
    path = _repo_root() / "systems/flux/flux/execution/nautilus_adapter.py"
    assert path.exists(), "nautilus adapter module should exist"
    return importlib.import_module("flux.execution.nautilus_adapter")


@pytest.fixture
def event_loop(session_event_loop):
    return session_event_loop


def _build_mock_client(event_loop):
    clock = LiveClock()
    trader_id = TestIdStubs.trader_id()
    msgbus = MessageBus(trader_id=trader_id, clock=clock)
    cache = TestComponentStubs.cache()
    instrument_provider = InstrumentProvider()
    instrument_provider.add(AUDUSD_SIM)
    instrument_provider.add(GBPUSD_SIM)
    return MockLiveExecutionClient(
        loop=event_loop,
        client_id=ClientId(SIM.value),
        venue=SIM,
        account_type=AccountType.CASH,
        base_currency=USD,
        instrument_provider=instrument_provider,
        msgbus=msgbus,
        cache=cache,
        clock=clock,
        oms_type=OmsType.NETTING,
    )


def test_managed_adapter_generate_mass_status_uses_open_only_and_filters_untracked_history(
    event_loop,
) -> None:
    nautilus_adapter = _load_nautilus_adapter_module()
    client = _build_mock_client(event_loop)

    tracked_report = OrderStatusReport(
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("TRACKED-OPEN-001"),
        venue_order_id=VenueOrderId("TRACKED-VENUE-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.ACCEPTED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(1_000),
        filled_qty=Quantity.zero(),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )
    untracked_report = OrderStatusReport(
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("UNTRACKED-CLOSED-001"),
        venue_order_id=VenueOrderId("UNTRACKED-VENUE-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(1_000),
        filled_qty=Quantity.from_int(1_000),
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )
    untracked_fill = FillReport(
        client_order_id=untracked_report.client_order_id,
        venue_order_id=untracked_report.venue_order_id,
        trade_id=TradeId("UNTRACKED-FILL-001"),
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        order_side=OrderSide.BUY,
        last_qty=Quantity.from_int(1_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(0, USD),
        liquidity_side=LiquiditySide.MAKER,
        report_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )
    position_report = PositionStatusReport(
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        position_side=PositionSide.LONG,
        quantity=Quantity.from_int(1_000),
        report_id=UUID4(),
        ts_last=2,
        ts_init=2,
    )

    client.add_order_status_report(tracked_report)
    client.add_order_status_report(untracked_report)
    client.add_fill_reports(untracked_report.venue_order_id, [untracked_fill])
    client.add_position_status_report(position_report)

    order_commands = []
    original_generate_order_status_reports = client.generate_order_status_reports

    async def capture_generate_order_status_reports(command):
        order_commands.append(command)
        return await original_generate_order_status_reports(command)

    client.generate_order_status_reports = capture_generate_order_status_reports

    adapter = nautilus_adapter.ControllerManagedExecutionClientAdapter(
        client=client,
        controller_scope_id="ibkr.hedge.main",
        managed_instrument_ids={AUDUSD_SIM.id},
        tracked_orders=(
            nautilus_adapter.ManagedOrderBinding(
                instrument_id=AUDUSD_SIM.id,
                client_order_id=tracked_report.client_order_id,
                venue_order_id=tracked_report.venue_order_id,
            ),
        ),
    )

    mass_status = event_loop.run_until_complete(adapter.generate_mass_status())

    assert adapter.supports_startup_historical_order_status_reports is False
    assert [command.instrument_id for command in order_commands] == [AUDUSD_SIM.id]
    assert [command.open_only for command in order_commands] == [True]
    assert set(mass_status.order_reports) == {tracked_report.venue_order_id}
    assert mass_status.fill_reports == {}
    assert mass_status.position_reports[AUDUSD_SIM.id] == [position_report]


def test_managed_adapter_passes_unmanaged_reports_through_unchanged(event_loop) -> None:
    nautilus_adapter = _load_nautilus_adapter_module()
    client = _build_mock_client(event_loop)

    report = OrderStatusReport(
        account_id=client.account_id,
        instrument_id=GBPUSD_SIM.id,
        client_order_id=ClientOrderId("UNMANAGED-ORDER-001"),
        venue_order_id=VenueOrderId("UNMANAGED-VENUE-001"),
        order_side=OrderSide.SELL,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.FILLED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(2_000),
        filled_qty=Quantity.from_int(2_000),
        avg_px=Decimal("1.00000"),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )
    fill = FillReport(
        client_order_id=report.client_order_id,
        venue_order_id=report.venue_order_id,
        trade_id=TradeId("UNMANAGED-FILL-001"),
        account_id=client.account_id,
        instrument_id=GBPUSD_SIM.id,
        order_side=OrderSide.SELL,
        last_qty=Quantity.from_int(2_000),
        last_px=Price.from_str("1.00000"),
        commission=Money(0, USD),
        liquidity_side=LiquiditySide.TAKER,
        report_id=UUID4(),
        ts_event=1,
        ts_init=1,
    )

    client.add_order_status_report(report)
    client.add_fill_reports(report.venue_order_id, [fill])

    adapter = nautilus_adapter.ControllerManagedExecutionClientAdapter(
        client=client,
        controller_scope_id="ibkr.hedge.main",
        managed_instrument_ids={AUDUSD_SIM.id},
        tracked_orders=(),
    )

    order_reports = event_loop.run_until_complete(
        adapter.generate_order_status_reports(
        GenerateOrderStatusReports(
            instrument_id=GBPUSD_SIM.id,
            start=None,
            end=None,
            open_only=False,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )
    fill_reports = event_loop.run_until_complete(
        adapter.generate_fill_reports(
        GenerateFillReports(
            instrument_id=GBPUSD_SIM.id,
            venue_order_id=None,
            start=None,
            end=None,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )
    targeted_report = event_loop.run_until_complete(
        adapter.generate_order_status_report(
        GenerateOrderStatusReport(
            instrument_id=GBPUSD_SIM.id,
            client_order_id=report.client_order_id,
            venue_order_id=report.venue_order_id,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )

    assert order_reports == [report]
    assert fill_reports == [fill]
    assert targeted_report == report


def test_managed_adapter_keeps_only_tracked_open_order_lineage_visible(event_loop) -> None:
    nautilus_adapter = _load_nautilus_adapter_module()
    client = _build_mock_client(event_loop)

    tracked_report = OrderStatusReport(
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("TRACKED-ORDER-001"),
        venue_order_id=VenueOrderId("TRACKED-VENUE-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.ACCEPTED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(500),
        filled_qty=Quantity.zero(),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )
    untracked_report = OrderStatusReport(
        account_id=client.account_id,
        instrument_id=AUDUSD_SIM.id,
        client_order_id=ClientOrderId("UNTRACKED-ORDER-001"),
        venue_order_id=VenueOrderId("UNTRACKED-VENUE-001"),
        order_side=OrderSide.BUY,
        order_type=OrderType.LIMIT,
        time_in_force=TimeInForce.GTC,
        order_status=OrderStatus.ACCEPTED,
        price=Price.from_str("1.00000"),
        quantity=Quantity.from_int(750),
        filled_qty=Quantity.zero(),
        report_id=UUID4(),
        ts_accepted=0,
        ts_last=0,
        ts_init=0,
    )

    client.add_order_status_report(tracked_report)
    client.add_order_status_report(untracked_report)

    adapter = nautilus_adapter.ControllerManagedExecutionClientAdapter(
        client=client,
        controller_scope_id="ibkr.hedge.main",
        managed_instrument_ids={AUDUSD_SIM.id},
        tracked_orders=(
            nautilus_adapter.ManagedOrderBinding(
                instrument_id=AUDUSD_SIM.id,
                client_order_id=tracked_report.client_order_id,
                venue_order_id=tracked_report.venue_order_id,
            ),
        ),
    )

    order_reports = event_loop.run_until_complete(
        adapter.generate_order_status_reports(
        GenerateOrderStatusReports(
            instrument_id=AUDUSD_SIM.id,
            start=None,
            end=None,
            open_only=True,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )
    tracked_targeted = event_loop.run_until_complete(
        adapter.generate_order_status_report(
        GenerateOrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            client_order_id=tracked_report.client_order_id,
            venue_order_id=tracked_report.venue_order_id,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )
    untracked_targeted = event_loop.run_until_complete(
        adapter.generate_order_status_report(
        GenerateOrderStatusReport(
            instrument_id=AUDUSD_SIM.id,
            client_order_id=untracked_report.client_order_id,
            venue_order_id=untracked_report.venue_order_id,
            command_id=UUID4(),
            ts_init=0,
        ),
        ),
    )

    assert order_reports == [tracked_report]
    assert tracked_targeted == tracked_report
    assert untracked_targeted is None
