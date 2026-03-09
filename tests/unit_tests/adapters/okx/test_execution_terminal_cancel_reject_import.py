from __future__ import annotations

from typing import get_type_hints

from nautilus_trader.adapters.okx.execution import OKXExecutionClient
from nautilus_trader.model.identifiers import ClientOrderId
from nautilus_trader.model.identifiers import VenueOrderId


def test_fill_matches_order_type_hints_require_venue_order_id_import() -> None:
    hints = get_type_hints(OKXExecutionClient._fill_matches_order)

    assert hints["client_order_id"] is ClientOrderId
    assert hints["venue_order_id"] == VenueOrderId | None
