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

import json

from nautilus_trader.adapters.polymarket.common.gamma_markets import normalize_gamma_market_to_clob_format


def test_normalize_gamma_market_to_clob_format() -> None:
    """
    Test that Gamma API market format is correctly normalized to CLOB API format.
    """
    # Arrange - Sample market data from Gamma API (based on user's example)
    gamma_market = {
        "question": "Fed rate hike in 2025?",
        "conditionId": "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221",
        "slug": "fed-rate-hike-in-2025",
        "resolutionSource": "",
        "endDate": "2025-12-10T12:00:00Z",
        "liquidity": "32462.67674",
        "startDate": "2024-12-29T22:50:33.584839Z",
        "image": "https://polymarket-upload.s3.us-east-2.amazonaws.com/will-the-fed-raise-interest-rates-in-2025-PQTEYZMvmAGr.jpg",
        "icon": "https://polymarket-upload.s3.us-east-2.amazonaws.com/will-the-fed-raise-interest-rates-in-2025-PQTEYZMvmAGr.jpg",
        "description": 'This market will resolve to "Yes" if the upper bound of the target federal funds rate is increased...',
        "outcomes": '["Yes", "No"]',
        "outcomePrices": '["0.014", "0.986"]',
        "volume": "660510.159796",
        "active": True,
        "closed": False,
        "marketMakerAddress": "",
        "createdAt": "2024-12-29T17:38:00.916304Z",
        "updatedAt": "2025-10-28T17:06:16.176352Z",
        "new": False,
        "featured": False,
        "submitted_by": "0x91430CaD2d3975766499717fA0D66A78D814E5c5",
        "archived": False,
        "resolvedBy": "0x6A9D222616C90FcA5754cd1333cFD9b7fb6a4F74",
        "restricted": True,
        "groupItemTitle": "",
        "groupItemThreshold": "0",
        "questionID": "0x8428884817cbc26422ec451101fcedfc5995907a8df6e5905bc29cd30d2867e7",
        "enableOrderBook": True,
        "orderPriceMinTickSize": 0.001,
        "orderMinSize": 5,
        "volumeNum": 660510.159796,
        "liquidityNum": 32462.67674,
        "endDateIso": "2025-12-10",
        "startDateIso": "2024-12-29",
        "hasReviewedDates": True,
        "volume24hr": 1173.1330750000002,
        "volume1wk": 27002.326095000008,
        "volume1mo": 113695.18619000004,
        "volume1yr": 660510.1597959978,
        "clobTokenIds": '["60487116984468020978247225474488676749601001829886755968952521846780452448915", "81104637750588840860328515305303028259865221573278091453716127842023614249200"]',
        "umaBond": "500",
        "umaReward": "5",
        "volume24hrClob": 1173.1330750000002,
        "volume1wkClob": 27002.326095000008,
        "volume1moClob": 113695.18619000004,
        "volume1yrClob": 660510.1597959978,
        "volumeClob": 660510.159796,
        "liquidityClob": 32462.67674,
        "acceptingOrders": True,
        "negRisk": False,
    }

    # Act
    normalized = normalize_gamma_market_to_clob_format(gamma_market)

    # Assert
    assert normalized["condition_id"] == "0x4319532e181605cb15b1bd677759a3bc7f7394b2fdf145195b700eeaedfd5221"
    assert normalized["question"] == "Fed rate hike in 2025?"
    assert normalized["minimum_tick_size"] == 0.001
    assert normalized["minimum_order_size"] == 5
    assert normalized["end_date_iso"] == "2025-12-10"
    assert normalized["maker_base_fee"] == 0
    assert normalized["taker_base_fee"] == 0
    assert normalized["active"] is True
    assert "_gamma_original" in normalized
    assert normalized["_gamma_original"] == gamma_market


def test_normalize_gamma_market_with_defaults() -> None:
    """
    Test normalization with missing optional fields uses defaults.
    """
    # Arrange - Minimal market data
    gamma_market = {
        "conditionId": "0x1234567890abcdef",
        "question": "Test market?",
        "endDateIso": "2025-12-31",
    }

    # Act
    normalized = normalize_gamma_market_to_clob_format(gamma_market)

    # Assert
    assert normalized["condition_id"] == "0x1234567890abcdef"
    assert normalized["question"] == "Test market?"
    assert normalized["minimum_tick_size"] == 0.001  # Default
    assert normalized["minimum_order_size"] == 5  # Default
    assert normalized["end_date_iso"] == "2025-12-31"
    assert normalized["maker_base_fee"] == 0  # Default
    assert normalized["taker_base_fee"] == 0  # Default
    assert normalized["active"] is False  # Default


def test_parse_clob_token_ids_and_outcomes() -> None:
    """
    Test parsing of clobTokenIds and outcomes from JSON strings.
    """
    # Arrange
    clob_token_ids_str = '["60487116984468020978247225474488676749601001829886755968952521846780452448915", "81104637750588840860328515305303028259865221573278091453716127842023614249200"]'
    outcomes_str = '["Yes", "No"]'

    # Act
    clob_token_ids = json.loads(clob_token_ids_str)
    outcomes = json.loads(outcomes_str)

    # Assert
    assert len(clob_token_ids) == 2
    assert len(outcomes) == 2
    assert clob_token_ids[0] == "60487116984468020978247225474488676749601001829886755968952521846780452448915"
    assert clob_token_ids[1] == "81104637750588840860328515305303028259865221573278091453716127842023614249200"
    assert outcomes[0] == "Yes"
    assert outcomes[1] == "No"

    # Test zipping them together
    token_outcome_pairs = list(zip(clob_token_ids, outcomes, strict=False))
    assert len(token_outcome_pairs) == 2
    assert token_outcome_pairs[0] == (
        "60487116984468020978247225474488676749601001829886755968952521846780452448915",
        "Yes",
    )
    assert token_outcome_pairs[1] == (
        "81104637750588840860328515305303028259865221573278091453716127842023614249200",
        "No",
    )
