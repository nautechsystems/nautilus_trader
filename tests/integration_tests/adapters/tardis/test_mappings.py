# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2025 Nautech Systems Pty Ltd. All rights reserved.
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

import pytest

from nautilus_trader.adapters.tardis.loaders import TardisCSVDataLoader
from nautilus_trader.core.nautilus_pyo3 import tardis_exchange_from_venue_str
from nautilus_trader.core.nautilus_pyo3 import tardis_exchange_to_venue_str
from nautilus_trader.core.nautilus_pyo3 import tardis_exchanges
from nautilus_trader.model import Venue
from tests.integration_tests.adapters.tardis.conftest import get_test_data_path


def test_all_tardis_exchanges_map_to_valid_venues():
    """
    Test that every Tardis exchange maps to a valid Nautilus venue.
    """
    exchanges = tardis_exchanges()
    for exchange_str in exchanges:
        venue_str = tardis_exchange_to_venue_str(exchange_str)
        try:
            venue = Venue(venue_str)
            assert venue is not None
        except ValueError as e:
            pytest.fail(
                f"Tardis exchange '{exchange_str}' maps to invalid Nautilus venue '{venue_str}': {e}",
            )


def test_venue_exchange_bidirectional_mapping():
    """
    Test bidirectional mapping between venues and exchanges.
    """
    exchanges = tardis_exchanges()
    venue_to_exchanges = {}

    # Build venue mapping from exchanges
    for exchange_str in exchanges:
        venue_str = tardis_exchange_to_venue_str(exchange_str)
        if venue_str not in venue_to_exchanges:
            venue_to_exchanges[venue_str] = []
        venue_to_exchanges[venue_str].append(exchange_str)

    # Test bidirectional mapping
    for venue_str, expected_exchanges in venue_to_exchanges.items():
        try:
            venue = Venue(venue_str)
            actual_exchanges = tardis_exchange_from_venue_str(venue.value)

            # Sort for comparison
            expected_exchanges.sort()
            actual_exchanges.sort()

            assert expected_exchanges == actual_exchanges, (
                f"Bidirectional mapping failed for venue '{venue_str}': "
                f"expected={expected_exchanges}, actual={actual_exchanges}"
            )
        except ValueError:
            # Some venue strings might not be valid Venue instances
            continue


@pytest.mark.parametrize("exchange", tardis_exchanges())
def test_exchange_to_venue_individual(exchange):
    """
    Test individual exchange to venue mapping.
    """
    venue_str = tardis_exchange_to_venue_str(exchange)
    assert venue_str is not None
    assert len(venue_str) > 0

    # Verify the venue can be created
    venue = Venue(venue_str)
    assert venue.value == venue_str


def test_exchange_strings_are_lowercase_kebab_case():
    """
    Test that exchange strings follow lowercase kebab-case convention.
    """
    exchanges = tardis_exchanges()
    for exchange in exchanges:
        # Should be lowercase
        assert exchange.islower(), f"Exchange '{exchange}' should be lowercase"

        # Should not contain underscores (kebab-case, not snake_case)
        assert "_" not in exchange, f"Exchange '{exchange}' should use dashes, not underscores"

        # Should only contain alphanumeric characters and dashes
        allowed_chars = set("abcdefghijklmnopqrstuvwxyz0123456789-")
        actual_chars = set(exchange)
        assert actual_chars.issubset(
            allowed_chars,
        ), f"Exchange '{exchange}' contains invalid characters: {actual_chars - allowed_chars}"


def test_venue_strings_are_uppercase_snake_case():
    """
    Test that venue strings follow uppercase snake_case convention.
    """
    exchanges = tardis_exchanges()
    venue_strings = set()

    for exchange in exchanges:
        venue_str = tardis_exchange_to_venue_str(exchange)
        venue_strings.add(venue_str)

    for venue_str in venue_strings:
        # Should be uppercase
        assert venue_str.isupper(), f"Venue '{venue_str}' should be uppercase"

        # Should only contain uppercase letters, numbers, and underscores
        allowed_chars = set("ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789_")
        actual_chars = set(venue_str)
        assert actual_chars.issubset(
            allowed_chars,
        ), f"Venue '{venue_str}' contains invalid characters: {actual_chars - allowed_chars}"


def test_no_empty_exchange_or_venue_mappings():
    """
    Test that no exchange maps to empty venue and vice versa.
    """
    exchanges = tardis_exchanges()

    for exchange in exchanges:
        venue_str = tardis_exchange_to_venue_str(exchange)
        assert venue_str != "", f"Exchange '{exchange}' maps to empty venue string"

        # Test reverse mapping
        mapped_exchanges = tardis_exchange_from_venue_str(venue_str)
        assert (
            exchange in mapped_exchanges
        ), f"Exchange '{exchange}' not found in reverse mapping for venue '{venue_str}'"


def test_exchange_inference_from_csv_data():
    """
    Test that venue is correctly inferred from CSV exchange field.
    """
    test_cases = [
        ("bitmex", "XBTUSD", "XBTUSD.BITMEX"),
        ("binance", "BTCUSDT", "BTCUSDT.BINANCE"),
        ("deribit", "BTC-PERPETUAL", "BTC-PERPETUAL.DERIBIT"),
    ]

    for exchange, symbol, expected_instrument_id in test_cases:
        venue_str = tardis_exchange_to_venue_str(exchange)
        constructed_id = f"{symbol}.{venue_str}"
        assert constructed_id == expected_instrument_id


def test_comprehensive_exchange_venue_coverage():
    """
    Test that we have consistent coverage across all mappings.
    """
    exchanges = tardis_exchanges()

    # Every exchange should map to a venue
    venue_count = 0
    for exchange in exchanges:
        venue_str = tardis_exchange_to_venue_str(exchange)
        assert venue_str != "", f"Exchange '{exchange}' maps to empty venue"
        venue_count += 1

    assert venue_count == len(exchanges), "Mismatch in exchange-to-venue mapping count"


def test_symbol_venue_consistency_with_stub_data():
    """
    Test symbol and venue consistency using stub data.
    """
    # Test trades stub data
    trades_path = get_test_data_path("trades_1.csv")
    loader = TardisCSVDataLoader()
    trades = loader.load_trades(trades_path)

    for trade in trades:
        # Verify instrument ID format
        id_parts = str(trade.instrument_id).split(".")
        assert len(id_parts) == 2, f"Invalid instrument ID format: {trade.instrument_id}"

        symbol, venue = id_parts
        assert symbol != "", "Symbol part should not be empty"
        assert venue != "", "Venue part should not be empty"

        # Verify venue is valid
        venue_obj = Venue(venue)
        assert venue_obj.value == venue

    # Test deltas stub data
    deltas_path = get_test_data_path("deltas_1.csv")
    deltas = loader.load_deltas(deltas_path)

    for delta in deltas:
        # Same checks for deltas
        id_parts = str(delta.instrument_id).split(".")
        assert len(id_parts) == 2, f"Invalid instrument ID format: {delta.instrument_id}"

        symbol, venue = id_parts
        venue_obj = Venue(venue)
        assert venue_obj.value == venue


def test_invalid_exchange_names():
    """
    Test handling of invalid exchange names.
    """
    invalid_exchanges = ["invalid-exchange", "nonexistent", ""]

    for invalid_exchange in invalid_exchanges:
        venue_str = tardis_exchange_to_venue_str(invalid_exchange)
        # Should return empty string for invalid exchanges
        assert venue_str == ""


def test_invalid_venue_names():
    """
    Test handling of invalid venue names.
    """
    invalid_venues = ["INVALID_VENUE", "NONEXISTENT"]

    for invalid_venue in invalid_venues:
        try:
            venue = Venue(invalid_venue)
            exchanges = tardis_exchange_from_venue_str(venue.value)
            # Should return empty list for unmapped venues
            assert exchanges == []
        except ValueError:
            # ValueError is acceptable for truly invalid venue strings
            continue
