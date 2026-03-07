from __future__ import annotations

from types import SimpleNamespace

from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId


def test_okx_execution_import_exposes_terminal_cancel_helpers() -> None:
    assert OKXExecutionClient._is_terminal_cancel_reject_reason(
        "s_code=51400, s_msg=filled, canceled or does not exist",
    )
    assert not OKXExecutionClient._is_terminal_cancel_reject_reason("temporarily unavailable")


def test_fill_matches_order_accepts_client_or_venue_order_id() -> None:
    report = SimpleNamespace(
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=VenueOrderId("V-1"),
    )

    assert OKXExecutionClient._fill_matches_order(
        report,
        client_order_id=ClientOrderId("O-1"),
        venue_order_id=None,
    )
    assert OKXExecutionClient._fill_matches_order(
        report,
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("V-1"),
    )
    assert not OKXExecutionClient._fill_matches_order(
        report,
        client_order_id=ClientOrderId("O-2"),
        venue_order_id=VenueOrderId("V-2"),
    )
