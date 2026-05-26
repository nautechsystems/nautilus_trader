# -------------------------------------------------------------------------------------------------
#  LMEX NautilusTrader Adapter — Tests
# -------------------------------------------------------------------------------------------------

"""
Tests for ``LmexInstrumentProvider`` instrument parsing.

Uses the real ``market_summary_sample.json`` fixture; no live API calls are made.
The ``LmexMarketHttpAPI`` is replaced with an ``AsyncMock``.
"""

from __future__ import annotations

import json
from decimal import Decimal
from unittest.mock import AsyncMock, MagicMock, patch

import msgspec
import pytest


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------


def _make_provider(market_summary_fixture: bytes):
    """
    Construct a ``LmexInstrumentProvider`` with a mocked ``LmexMarketHttpAPI``.

    The mock's ``get_market_summary`` decodes the fixture and returns the list.
    """
    from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
    from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
    from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary

    decoded = msgspec.json.decode(market_summary_fixture, type=list[LmexMarketSummary])

    mock_api = MagicMock(spec=LmexMarketHttpAPI)
    mock_api.get_market_summary = AsyncMock(return_value=decoded)

    with patch(
        "nautilus_trader.adapters.lmex.providers.Logger",
        return_value=MagicMock(),
    ):
        provider = LmexInstrumentProvider(market_api=mock_api)

    return provider, mock_api


# ---------------------------------------------------------------------------
# _decimal_places unit tests
# ---------------------------------------------------------------------------


class TestDecimalPlaces:
    """Tests for the ``_decimal_places`` helper function."""

    def _dp(self, value: float) -> int:
        from nautilus_trader.adapters.lmex.providers import _decimal_places

        return _decimal_places(value)

    def test_one_decimal_place(self) -> None:
        """0.1 → 1 decimal place."""
        assert self._dp(0.1) == 1

    def test_two_decimal_places(self) -> None:
        """0.01 → 2 decimal places."""
        assert self._dp(0.01) == 2

    def test_five_decimal_places(self) -> None:
        """1e-05 → 5 decimal places (BTC-USD minSizeIncrement)."""
        assert self._dp(1e-05) == 5

    def test_four_decimal_places(self) -> None:
        """0.0001 → 4 decimal places (ETH-USD minSizeIncrement)."""
        assert self._dp(0.0001) == 4

    def test_zero_decimal_places(self) -> None:
        """Whole-number increment → 0 decimal places."""
        assert self._dp(1.0) == 0

    def test_non_positive_returns_zero(self) -> None:
        """Non-positive input is clamped to 0 decimal places."""
        assert self._dp(0.0) == 0
        assert self._dp(-1.0) == 0


# ---------------------------------------------------------------------------
# Provider loading tests
# ---------------------------------------------------------------------------


class TestLmexInstrumentProviderLoading:
    """Tests for ``load_all_async`` instrument loading."""

    @pytest.mark.asyncio
    async def test_load_all_async_loads_all_active_spot(
        self, market_summary_fixture: bytes
    ) -> None:
        """All four active spot instruments in the fixture are loaded."""
        provider, _ = _make_provider(market_summary_fixture)
        await provider.load_all_async()

        instruments = provider.get_all()
        assert len(instruments) == 4

    @pytest.mark.asyncio
    async def test_btcusd_in_provider(self, market_summary_fixture: bytes) -> None:
        """BTC-USD.LMEX is present after loading."""
        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        provider, _ = _make_provider(market_summary_fixture)
        await provider.load_all_async()

        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)
        instrument = provider.find(iid)
        assert instrument is not None

    @pytest.mark.asyncio
    async def test_filters_skip_single_symbol(
        self, market_summary_fixture: bytes
    ) -> None:
        """
        When ``get_market_summary`` is called with a ``symbol`` filter it only
        returns that one entry; the provider loads exactly one instrument.
        """
        from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
        from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
        from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary

        all_summaries = msgspec.json.decode(
            market_summary_fixture, type=list[LmexMarketSummary]
        )
        btc_only = [s for s in all_summaries if s.symbol == "BTC-USD"]

        mock_api = MagicMock(spec=LmexMarketHttpAPI)
        mock_api.get_market_summary = AsyncMock(return_value=btc_only)

        with patch(
            "nautilus_trader.adapters.lmex.providers.Logger",
            return_value=MagicMock(),
        ):
            provider = LmexInstrumentProvider(market_api=mock_api)

        await provider.load_all_async(filters={"symbol": "BTC-USD"})

        instruments = provider.get_all()
        assert len(instruments) == 1

    @pytest.mark.asyncio
    async def test_futures_skipped_with_spot_only_filter(
        self, market_summary_fixture: bytes
    ) -> None:
        """
        Instruments with ``futures=True`` are skipped when ``spot_only=True``
        (the default).
        """
        from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
        from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
        from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary

        # Inject a synthetic futures instrument
        raw = json.loads(market_summary_fixture)
        raw.append(
            {
                **raw[0],
                "symbol": "BTC-USD-PERP",
                "futures": True,
            }
        )
        modified = msgspec.json.encode(raw)
        summaries = msgspec.json.decode(modified, type=list[LmexMarketSummary])

        mock_api = MagicMock(spec=LmexMarketHttpAPI)
        mock_api.get_market_summary = AsyncMock(return_value=summaries)

        with patch(
            "nautilus_trader.adapters.lmex.providers.Logger",
            return_value=MagicMock(),
        ):
            provider = LmexInstrumentProvider(market_api=mock_api)

        await provider.load_all_async()  # spot_only=True by default

        symbols = [i.id.symbol.value for i in provider.get_all().values()]
        assert "BTC-USD-PERP" not in symbols
        assert len(provider.get_all()) == 4  # original 4 spot pairs only


# ---------------------------------------------------------------------------
# Instrument field precision tests
# ---------------------------------------------------------------------------


class TestLmexInstrumentPrecision:
    """Tests that parsed ``CurrencyPair`` fields match LMEX market data."""

    @pytest.fixture(scope="class")
    def btc_usd(self, market_summary_fixture: bytes):
        """Return the parsed BTC-USD CurrencyPair (loaded synchronously)."""
        import asyncio

        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        provider, _ = _make_provider(market_summary_fixture)
        asyncio.get_event_loop().run_until_complete(provider.load_all_async())

        iid = InstrumentId(Symbol("BTC-USD"), LMEX_VENUE)
        return provider.find(iid)

    @pytest.fixture(scope="class")
    def eth_usd(self, market_summary_fixture: bytes):
        """Return the parsed ETH-USD CurrencyPair."""
        import asyncio

        from nautilus_trader.adapters.lmex.constants import LMEX_VENUE
        from nautilus_trader.model.identifiers import InstrumentId, Symbol

        provider, _ = _make_provider(market_summary_fixture)
        asyncio.get_event_loop().run_until_complete(provider.load_all_async())

        iid = InstrumentId(Symbol("ETH-USD"), LMEX_VENUE)
        return provider.find(iid)

    def test_btcusd_price_precision(self, btc_usd) -> None:
        """BTC-USD minPriceIncrement=0.1 → price_precision=1."""
        assert btc_usd.price_precision == 1

    def test_btcusd_size_precision(self, btc_usd) -> None:
        """BTC-USD minSizeIncrement=1e-05 → size_precision=5."""
        assert btc_usd.size_precision == 5

    def test_btcusd_price_increment(self, btc_usd) -> None:
        """BTC-USD price_increment matches minPriceIncrement=0.1."""
        assert float(btc_usd.price_increment) == pytest.approx(0.1)

    def test_btcusd_size_increment(self, btc_usd) -> None:
        """BTC-USD size_increment matches minSizeIncrement=1e-05."""
        assert float(btc_usd.size_increment) == pytest.approx(1e-05)

    def test_btcusd_min_quantity(self, btc_usd) -> None:
        """BTC-USD min_quantity matches minOrderSize=1e-05."""
        assert btc_usd.min_quantity is not None
        assert float(btc_usd.min_quantity) == pytest.approx(1e-05)

    def test_btcusd_max_quantity(self, btc_usd) -> None:
        """BTC-USD max_quantity matches maxOrderSize=100."""
        assert btc_usd.max_quantity is not None
        assert float(btc_usd.max_quantity) == pytest.approx(100.0)

    def test_btcusd_base_currency(self, btc_usd) -> None:
        """BTC-USD base currency is BTC."""
        assert btc_usd.base_currency.code == "BTC"

    def test_btcusd_quote_currency(self, btc_usd) -> None:
        """BTC-USD quote currency is USD."""
        assert btc_usd.quote_currency.code == "USD"

    def test_btcusd_venue(self, btc_usd) -> None:
        """BTC-USD instrument ID has LMEX venue."""
        assert btc_usd.id.venue.value == "LMEX"

    def test_ethusd_price_precision(self, eth_usd) -> None:
        """ETH-USD minPriceIncrement=0.01 → price_precision=2."""
        assert eth_usd.price_precision == 2

    def test_ethusd_size_precision(self, eth_usd) -> None:
        """ETH-USD minSizeIncrement=0.0001 → size_precision=4."""
        assert eth_usd.size_precision == 4

    def test_ethusd_max_quantity(self, eth_usd) -> None:
        """ETH-USD maxOrderSize=2000."""
        assert eth_usd.max_quantity is not None
        assert float(eth_usd.max_quantity) == pytest.approx(2000.0)

    def test_instrument_info_dict(self, btc_usd) -> None:
        """``info`` dict contains LMEX-specific fields."""
        assert btc_usd.info["symbol"] == "BTC-USD"
        assert btc_usd.info["active"] is True
        assert btc_usd.info["futures"] is False


# ---------------------------------------------------------------------------
# Provider error-handling tests
# ---------------------------------------------------------------------------


class TestLmexInstrumentProviderErrors:
    """Tests for graceful handling of bad data during loading."""

    @pytest.mark.asyncio
    async def test_invalid_instrument_skipped_with_warning(self) -> None:
        """
        If ``_parse_instrument`` raises for one entry, it is skipped and the
        remaining instruments are still loaded.
        """
        from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
        from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
        from nautilus_trader.adapters.lmex.schemas.market import LmexMarketSummary

        # Two valid entries: BTC-USD and a zero-increment entry that will fail
        good_summary = msgspec.json.decode(
            b'{"symbol":"BTC-USD","last":76731.9,"lowestAsk":76734.7,'
            b'"highestBid":76731.9,"percentageChange":-1.07,'
            b'"volume":172316691.33,"high24Hr":77869.9,"low24Hr":76436.3,'
            b'"base":"BTC","quote":"USD","active":true,"size":2233.11,'
            b'"minValidPrice":0.1,"minPriceIncrement":0.1,"minOrderSize":1e-05,'
            b'"maxOrderSize":100,"minSizeIncrement":1e-05,"openInterest":0,'
            b'"openInterestUSD":0,"contractStart":0,"contractEnd":0,'
            b'"timeBasedContract":false,"openTime":0,"closeTime":0,'
            b'"startMatching":0,"inactiveTime":0,"fundingRate":0,'
            b'"contractSize":0,"maxPosition":0,"minRiskLimit":0,'
            b'"maxRiskLimit":0,"availableSettlement":null,"futures":false,'
            b'"isMarketOpenToOtc":true,"isMarketOpenToSpot":true}',
            type=LmexMarketSummary,
        )

        mock_api = MagicMock(spec=LmexMarketHttpAPI)

        # First call returns one valid summary
        mock_api.get_market_summary = AsyncMock(return_value=[good_summary])

        mock_log = MagicMock()
        with patch(
            "nautilus_trader.adapters.lmex.providers.Logger",
            return_value=mock_log,
        ):
            provider = LmexInstrumentProvider(market_api=mock_api)

        # Patch _parse_instrument to raise for the good entry too, simulating failure
        with patch.object(
            provider,
            "_parse_instrument",
            side_effect=ValueError("parse error"),
        ):
            await provider.load_all_async()

        # Nothing was loaded (all raised), but no exception was propagated
        assert len(provider.get_all()) == 0
        # Warning was logged
        mock_log.warning.assert_called_once()
        assert "Failed to parse" in mock_log.warning.call_args[0][0]

    @pytest.mark.asyncio
    async def test_load_ids_wrong_venue_raises(self) -> None:
        """``load_ids_async`` raises ``ValueError`` when venue is not LMEX."""
        from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
        from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider
        from nautilus_trader.model.identifiers import InstrumentId, Symbol, Venue

        mock_api = MagicMock(spec=LmexMarketHttpAPI)
        with patch(
            "nautilus_trader.adapters.lmex.providers.Logger",
            return_value=MagicMock(),
        ):
            provider = LmexInstrumentProvider(market_api=mock_api)

        wrong_venue_id = InstrumentId(Symbol("BTC-USD"), Venue("BINANCE"))
        with pytest.raises(ValueError, match="LMEX"):
            await provider.load_ids_async([wrong_venue_id])

    @pytest.mark.asyncio
    async def test_load_ids_empty_list_is_noop(self) -> None:
        """Calling ``load_ids_async`` with an empty list is a no-op."""
        from nautilus_trader.adapters.lmex.http.market import LmexMarketHttpAPI
        from nautilus_trader.adapters.lmex.providers import LmexInstrumentProvider

        mock_api = MagicMock(spec=LmexMarketHttpAPI)
        mock_log = MagicMock()
        with patch(
            "nautilus_trader.adapters.lmex.providers.Logger",
            return_value=mock_log,
        ):
            provider = LmexInstrumentProvider(market_api=mock_api)

        await provider.load_ids_async([])  # must not raise

        mock_api.get_market_summary.assert_not_called()
        mock_log.warning.assert_called_once()
