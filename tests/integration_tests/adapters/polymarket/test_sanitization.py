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

import copy

import pytest

from nautilus_trader.adapters.polymarket.common.sanitization import extract_resolution_metadata
from nautilus_trader.adapters.polymarket.common.sanitization import sanitize_info_for_simulation


_RESOLVED_INFO = {
    "condition_id": "0xcond",
    "question": "Will it rain?",
    "closed": True,
    "closedTime": "2025-01-01T00:00:00Z",
    "umaResolutionStatus": "resolved",
    "tokens": [
        {"token_id": "1", "outcome": "Yes", "winner": True, "price": 0.99},
        {"token_id": "2", "outcome": "No", "winner": False, "price": 0.01},
    ],
}


@pytest.mark.parametrize(
    ("info", "expected"),
    [
        (None, {}),
        ({}, {}),
        ({"condition_id": "0xcond"}, {}),
        ({"closed": True}, {"closed": True}),
        (
            {"closedTime": "2025-01-01T00:00:00Z"},
            {"closedTime": "2025-01-01T00:00:00Z"},
        ),
        (
            {"umaResolutionStatus": "resolved"},
            {"umaResolutionStatus": "resolved"},
        ),
        (
            {"uma_resolution_status": "resolved"},
            {"uma_resolution_status": "resolved"},
        ),
        (
            {"resolutionSource": "0x...uma"},
            {"resolutionSource": "0x...uma"},
        ),
        (
            {"resolution_source": "0x...uma"},
            {"resolution_source": "0x...uma"},
        ),
    ],
)
def test_extract_resolution_metadata_top_level(info, expected):
    # Act
    result = extract_resolution_metadata(info)

    # Assert
    assert result == expected


def test_extract_resolution_metadata_per_token_winner():
    # Arrange
    info = {
        "tokens": [
            {"outcome": "Yes", "winner": True},
            {"outcome": "No"},
        ],
    }

    # Act
    result = extract_resolution_metadata(info)

    # Assert - only the winning entry surfaces (the non-winner has no
    # resolution flag, so it is omitted to keep the slice minimal)
    assert result == {"tokens": [{"outcome": "Yes", "winner": True}]}


def test_extract_resolution_metadata_combined_top_and_tokens():
    # Act
    result = extract_resolution_metadata(_RESOLVED_INFO)

    # Assert
    assert result == {
        "closed": True,
        "closedTime": "2025-01-01T00:00:00Z",
        "umaResolutionStatus": "resolved",
        "tokens": [
            {"outcome": "Yes", "winner": True},
            {"outcome": "No", "winner": False},
        ],
    }


def test_extract_resolution_metadata_skips_non_mapping_token_entries():
    # Arrange - tolerate junk entries the venue may emit
    info = {
        "tokens": [
            {"outcome": "Yes", "winner": True},
            "garbage",
            {"outcome": "No"},
        ],
    }

    # Act
    result = extract_resolution_metadata(info)

    # Assert
    assert result == {"tokens": [{"outcome": "Yes", "winner": True}]}


@pytest.mark.parametrize(
    "info",
    [None, {}],
)
def test_sanitize_info_empty_inputs(info):
    # Act
    result = sanitize_info_for_simulation(info)

    # Assert
    assert result == {}


def test_sanitize_info_strips_top_level_resolution_keys():
    # Act
    result = sanitize_info_for_simulation(_RESOLVED_INFO)

    # Assert - resolution keys gone, non-resolution keys preserved
    assert "closed" not in result
    assert "closedTime" not in result
    assert "umaResolutionStatus" not in result
    assert result["condition_id"] == "0xcond"
    assert result["question"] == "Will it rain?"


def test_sanitize_info_strips_per_token_winner():
    # Act
    result = sanitize_info_for_simulation(_RESOLVED_INFO)

    # Assert - winner removed, other token fields kept
    for token in result["tokens"]:
        assert "winner" not in token
    assert result["tokens"][0]["outcome"] == "Yes"
    assert result["tokens"][0]["price"] == 0.99


def test_sanitize_info_does_not_mutate_input():
    # Arrange
    original = copy.deepcopy(_RESOLVED_INFO)

    # Act
    sanitize_info_for_simulation(_RESOLVED_INFO)

    # Assert - input still has the resolution fields
    assert original == _RESOLVED_INFO


def test_extract_and_sanitize_partition_resolution_fields():
    # Arrange
    info = copy.deepcopy(_RESOLVED_INFO)

    # Act
    metadata = extract_resolution_metadata(info)
    sanitized = sanitize_info_for_simulation(info)

    # Assert - sanitized has none of the resolution keys, metadata has all
    # of them; the union covers every key in the original
    sanitized_keys = set(sanitized.keys())
    metadata_keys = set(metadata.keys())
    assert sanitized_keys.isdisjoint(metadata_keys - {"tokens"})
    for token in sanitized["tokens"]:
        assert "winner" not in token


def test_sanitize_info_preserves_non_mapping_token_entries():
    # Arrange - guard against accidental coercion of unexpected payloads
    info = {"tokens": [{"outcome": "Yes", "winner": True}, "junk"]}

    # Act
    result = sanitize_info_for_simulation(info)

    # Assert
    assert result["tokens"][0] == {"outcome": "Yes"}
    assert result["tokens"][1] == "junk"
