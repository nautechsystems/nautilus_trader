# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2026 Nautech Systems Pty Ltd. All rights reserved.
#  https://nautechsystems.io
# -------------------------------------------------------------------------------------------------

from decimal import Decimal

import pytest

from nautilus_trader.adapters.bitget.constants import BITGET_VENUE
from nautilus_trader.adapters.bitget.providers import BitgetInstrumentProvider
from nautilus_trader.model.currencies import BTC
from nautilus_trader.model.currencies import USDT
from nautilus_trader.model.enums import AccountType
from nautilus_trader.model.events import AccountState
from nautilus_trader.model.identifiers import AccountId
from nautilus_trader.model.identifiers import InstrumentId
from nautilus_trader.model.identifiers import Symbol
from nautilus_trader.model.identifiers import Venue
from nautilus_trader.model.instruments.currency_pair import CurrencyPair
from nautilus_trader.model.objects import AccountBalance
from nautilus_trader.model.objects import Money
from nautilus_trader.model.objects import Price
from nautilus_trader.model.objects import Quantity
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


class DummyBitgetClient:
    """Test double for Bitget HTTP client."""


async def _noop_request_instruments() -> list[object]:
    return []


def dummy_client_with_request() -> DummyBitgetClient:
    """Return a dummy client exposing request_instruments."""
    client = DummyBitgetClient()
    client.request_instruments = _noop_request_instruments  # type: ignore[attr-defined]
    return client


def dummy_provider() -> BitgetInstrumentProvider:
    """Return a Bitget instrument provider test instance."""
    return BitgetInstrumentProvider(client=DummyBitgetClient())


@pytest.fixture
def venue() -> Venue:
    return BITGET_VENUE


@pytest.fixture
def instrument() -> CurrencyPair:
    return CurrencyPair(
        instrument_id=InstrumentId(Symbol("BTCUSDT"), BITGET_VENUE),
        raw_symbol=Symbol("BTCUSDT"),
        base_currency=BTC,
        quote_currency=USDT,
        price_precision=2,
        size_precision=6,
        price_increment=Price.from_str("0.01"),
        size_increment=Quantity.from_str("0.000001"),
        ts_event=0,
        ts_init=0,
        maker_fee=Decimal("0.001"),
        taker_fee=Decimal("0.001"),
    )


@pytest.fixture
def account_state(venue: Venue) -> AccountState:
    account_id = AccountId(f"{venue.value}-123")
    return AccountState(
        account_id=account_id,
        account_type=AccountType.CASH,
        base_currency=USDT,
        reported=True,
        balances=[
            AccountBalance(
                total=Money(100_000, USDT),
                locked=Money(0, USDT),
                free=Money(100_000, USDT),
            ),
        ],
        margins=[],
        info={},
        event_id=TestIdStubs.uuid(),
        ts_event=0,
        ts_init=0,
    )


@pytest.fixture
def data_client() -> None:
    return None


@pytest.fixture
def exec_client() -> None:
    return None
