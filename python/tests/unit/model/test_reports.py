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

from nautilus_trader.core import UUID4
from nautilus_trader.model import AccountId
from nautilus_trader.model import ClientId
from nautilus_trader.model import ClientOrderId
from nautilus_trader.model import ExecutionMassStatus
from nautilus_trader.model import FillReport
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Money
from nautilus_trader.model import OrderInitialized
from nautilus_trader.model import OrderSnapshot
from nautilus_trader.model import OrderStatusReport
from nautilus_trader.model import OrderType
from nautilus_trader.model import Position
from nautilus_trader.model import PositionAdjusted
from nautilus_trader.model import PositionAdjustmentType
from nautilus_trader.model import PositionChanged
from nautilus_trader.model import PositionClosed
from nautilus_trader.model import PositionId
from nautilus_trader.model import PositionOpened
from nautilus_trader.model import PositionSnapshot
from nautilus_trader.model import PositionStatusReport
from nautilus_trader.model import Quantity
from nautilus_trader.model import StrategyId
from nautilus_trader.model import TraderId
from nautilus_trader.model import TriggerType
from nautilus_trader.model import Venue
from nautilus_trader.model import VenueOrderId
from tests.providers import TestInstrumentProvider
from tests.unit.model.factories import make_fill_report
from tests.unit.model.factories import make_market_order_snapshot_values
from tests.unit.model.factories import make_order_initialized
from tests.unit.model.factories import make_order_status_report
from tests.unit.model.factories import make_position_fill
from tests.unit.model.factories import make_position_status_report


def test_fill_report_to_dict_and_from_dict_roundtrip(audusd_id):
    report = make_fill_report(audusd_id)

    data = report.to_dict()
    restored = FillReport.from_dict(data)

    assert data["type"] == "FillReport"
    assert restored == report
    assert restored.client_order_id == ClientOrderId("O-1")
    assert restored.venue_position_id == PositionId("P-1")


def test_order_status_report_to_dict_and_from_dict_roundtrip(audusd_id):
    report = make_order_status_report(audusd_id, include_optionals=False)

    data = report.to_dict()
    restored = OrderStatusReport.from_dict(data)
    report_with_optionals = make_order_status_report(audusd_id, include_optionals=True)

    assert data["type"] == "OrderStatusReport"
    assert report.is_open
    assert restored.venue_order_id == VenueOrderId("1")
    assert restored.filled_qty == Quantity.from_int(25_000)
    assert report_with_optionals.linked_order_ids == [ClientOrderId("O-2")]
    assert report_with_optionals.avg_px == Decimal("1.00005")
    assert report_with_optionals.post_only is True
    assert report_with_optionals.trigger_type == TriggerType.BID_ASK


def test_execution_mass_status_adds_reports_and_roundtrips(audusd_id):
    order_report = make_order_status_report(audusd_id, include_optionals=False)
    fill_report = make_fill_report(audusd_id)
    position_report = make_position_status_report(audusd_id)

    status = ExecutionMassStatus(
        client_id=ClientId("CID"),
        account_id=AccountId("SIM-001"),
        venue=Venue("SIM"),
        ts_init=77,
    )
    status.add_order_reports([order_report])
    status.add_fill_reports([fill_report])
    status.add_position_reports([position_report])

    data = status.to_dict()
    restored = ExecutionMassStatus.from_dict(data)

    assert data["type"] == "ExecutionMassStatus"
    assert list(data["order_reports"].keys()) == ["1"]
    assert list(data["fill_reports"].keys()) == ["1"]
    assert list(data["position_reports"].keys()) == ["AUD/USD.SIM"]
    assert list(restored.order_reports.keys()) == [VenueOrderId("1")]
    assert list(restored.fill_reports.keys()) == [VenueOrderId("1")]
    assert list(restored.position_reports.keys()) == [InstrumentId.from_str("AUD/USD.SIM")]


def test_order_initialized_to_dict_and_from_dict_roundtrip(audusd_id):
    event = make_order_initialized(audusd_id)

    restored = OrderInitialized.from_dict(event.to_dict())

    assert restored.order_type == OrderType.STOP_LIMIT


def test_order_snapshot_from_dict_returns_snapshot_instance(audusd_id):
    snapshot = OrderSnapshot.from_dict(make_market_order_snapshot_values(audusd_id))

    assert type(snapshot).__name__ == "OrderSnapshot"


def test_position_adjusted_to_dict_and_from_dict_roundtrip(audusd_id):
    event = PositionAdjusted(
        trader_id=TraderId("TRADER-001"),
        strategy_id=StrategyId("S-001"),
        instrument_id=audusd_id,
        position_id=PositionId("P-1"),
        account_id=AccountId("SIM-001"),
        adjustment_type=PositionAdjustmentType.FUNDING,
        quantity_change=Decimal("100.5"),
        pnl_change=Money.from_str("12.00 USD"),
        reason="funding",
        event_id=UUID4(),
        ts_event=10,
        ts_init=11,
    )

    restored = PositionAdjusted.from_dict(event.to_dict())

    assert restored.adjustment_type == PositionAdjustmentType.FUNDING
    assert restored.quantity_change == Decimal("100.5")
    assert restored.pnl_change == Money.from_str("12.00 USD")
    assert restored.reason == "funding"


def test_position_status_report_properties_and_roundtrip(audusd_id):
    report = make_position_status_report(audusd_id)
    restored = PositionStatusReport.from_dict(report.to_dict())

    assert report.is_long
    assert not report.is_short
    assert not report.is_flat
    assert report.quantity == Quantity.from_int(100_000)
    assert report.avg_px_open == Decimal("1.00010")
    assert restored.venue_position_id == PositionId("P-1")


def test_position_snapshot_from_dict_returns_snapshot_instance():
    instrument = TestInstrumentProvider.audusd_sim()
    fill = make_position_fill(instrument)
    position = Position(instrument=instrument, fill=fill)
    values = position.to_dict()
    values["unrealized_pnl"] = None

    snapshot = PositionSnapshot.from_dict(values)

    assert type(snapshot).__name__ == "PositionSnapshot"


def test_position_event_classes_expose_create_surface():
    assert hasattr(PositionOpened, "position_id")
    assert hasattr(PositionOpened, "quantity")
    assert hasattr(PositionChanged, "peak_quantity")
    assert hasattr(PositionChanged, "realized_pnl")
    assert hasattr(PositionClosed, "closing_order_id")
    assert hasattr(PositionClosed, "ts_closed")
