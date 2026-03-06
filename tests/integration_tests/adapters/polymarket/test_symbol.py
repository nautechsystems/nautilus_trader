import pytest

from nautilus_trader.adapters.polymarket.common.constants import POLYMARKET_VENUE
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_condition_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_instrument_id
from nautilus_trader.adapters.polymarket.common.symbol import get_polymarket_token_id
from nautilus_trader.model.identifiers import InstrumentId


class TestPolymarketSymbol:
    def test_get_polymarket_instrument_id(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"

        result = get_polymarket_instrument_id(condition_id, token_id)

        expected = InstrumentId.from_str(f"{condition_id}-{token_id}.{POLYMARKET_VENUE}")
        assert result == expected

    def test_get_polymarket_condition_id_valid(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
        instrument_id = get_polymarket_instrument_id(condition_id, token_id)

        result = get_polymarket_condition_id(instrument_id)

        assert result == condition_id

    def test_get_polymarket_token_id_valid(self):
        condition_id = "0xdd22472e552920b8438158ea7238bfadfa4f736aa4cee91a6b86c39ead110917"
        token_id = "21742633143463906290569050155826241533067272736897614950488156847949938836455"
        instrument_id = get_polymarket_instrument_id(condition_id, token_id)

        result = get_polymarket_token_id(instrument_id)

        assert result == token_id

    def test_get_polymarket_condition_id_no_dash_raises_error(self):
        instrument_id = InstrumentId.from_str(f"invalid_no_dash.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_no_dash_raises_error(self):
        instrument_id = InstrumentId.from_str(f"invalid_no_dash.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)

    def test_get_polymarket_condition_id_too_many_dashes_raises_error(self):
        instrument_id = InstrumentId.from_str(f"too-many-dashes.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_too_many_dashes_raises_error(self):
        instrument_id = InstrumentId.from_str(f"too-many-dashes.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)

    def test_get_polymarket_condition_id_missing_condition_raises_error(self):
        instrument_id = InstrumentId.from_str(f"-token_id.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_condition_id(instrument_id)

    def test_get_polymarket_token_id_missing_token_raises_error(self):
        instrument_id = InstrumentId.from_str(f"condition_id-.{POLYMARKET_VENUE}")

        with pytest.raises(ValueError, match="Invalid Polymarket instrument ID format"):
            get_polymarket_token_id(instrument_id)
