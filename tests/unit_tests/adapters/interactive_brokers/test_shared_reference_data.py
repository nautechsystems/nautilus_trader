from __future__ import annotations

from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    build_shared_reference_quote_tick,
)
from nautilus_trader.adapters.interactive_brokers.shared_reference.data import (
    shared_reference_quote_channel,
)
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity


def test_shared_reference_quote_channel_is_profile_scoped() -> None:
    assert shared_reference_quote_channel(
        profile_id="equities",
        account_scope_id="ibkr.reference.main",
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
    ) == "flux:v1:profile:market:last:equities:ibkr.reference.main:ibkr:AAPL.NASDAQ:changed"


def test_build_shared_reference_quote_tick_translates_snapshot_payload() -> None:
    quote = build_shared_reference_quote_tick(
        payload={
            "instrument_id": "AAPL.NASDAQ",
            "bid": 190.25,
            "ask": 190.5,
            "bid_size": 7,
            "ask_size": 9,
            "ts_event_ms": 9_900,
            "route": "SMART",
            "session": "RTH",
        },
        instrument_id=InstrumentId.from_str("AAPL.NASDAQ"),
        ts_init_ns=10_000_000_000,
    )

    assert quote.instrument_id == InstrumentId.from_str("AAPL.NASDAQ")
    assert quote.bid_price == Price.from_str("190.25")
    assert quote.ask_price == Price.from_str("190.50")
    assert quote.bid_size == Quantity.from_int(7)
    assert quote.ask_size == Quantity.from_int(9)
    assert quote.ts_event == 9_900_000_000
    assert quote.ts_init == 10_000_000_000
