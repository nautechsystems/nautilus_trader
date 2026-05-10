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

import pytest

from nautilus_trader.adapters.databento.data_utils import databento_definition_dates
from nautilus_trader.adapters.databento.data_utils import next_day


class TestNextDay:
    def test_next_day_basic(self) -> None:
        # Arrange
        date_str = "2024-01-15"

        # Act
        result = next_day(date_str)

        # Assert
        assert result == "2024-01-16"

    def test_next_day_month_boundary(self) -> None:
        # Arrange
        date_str = "2024-01-31"

        # Act
        result = next_day(date_str)

        # Assert
        assert result == "2024-02-01"

    def test_next_day_year_boundary(self) -> None:
        # Arrange
        date_str = "2024-12-31"

        # Act
        result = next_day(date_str)

        # Assert
        assert result == "2025-01-01"

    def test_next_day_leap_year_february(self) -> None:
        # Arrange
        date_str = "2024-02-28"

        # Act
        result = next_day(date_str)

        # Assert
        assert result == "2024-02-29"  # 2024 is a leap year

    def test_next_day_leap_year_february_29(self) -> None:
        # Arrange
        date_str = "2024-02-29"

        # Act
        result = next_day(date_str)

        # Assert
        assert result == "2024-03-01"


class TestDatabentoDefinitionDates:
    def test_with_start_time_only_returns_single_day_range(self) -> None:
        # Arrange
        start_time = "2024-01-15T10:30:00Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time)

        # Assert
        assert start_date == "2024-01-15"
        assert end_date == "2024-01-16"  # next_day applied for half-open interval

    def test_with_start_and_end_time_returns_full_range(self) -> None:
        # Arrange
        start_time = "2024-01-15T10:30:00Z"
        end_time = "2024-01-20T15:45:00Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == "2024-01-15"
        assert end_date == "2024-01-21"  # next_day applied for half-open interval

    def test_with_same_day_range(self) -> None:
        # Arrange
        start_time = "2024-01-15T00:00:00Z"
        end_time = "2024-01-15T23:59:59Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == "2024-01-15"
        assert end_date == "2024-01-16"  # next_day applied for half-open interval

    def test_with_month_boundary(self) -> None:
        # Arrange
        start_time = "2024-01-28T00:00:00Z"
        end_time = "2024-01-31T23:59:59Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == "2024-01-28"
        assert end_date == "2024-02-01"  # next_day applied for half-open interval

    def test_with_year_boundary(self) -> None:
        # Arrange
        start_time = "2024-12-30T00:00:00Z"
        end_time = "2024-12-31T23:59:59Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == "2024-12-30"
        assert end_date == "2025-01-01"  # next_day applied for half-open interval

    def test_with_multi_year_range(self) -> None:
        # Arrange
        start_time = "2020-01-01T00:00:00Z"
        end_time = "2024-06-15T00:00:00Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == "2020-01-01"
        assert end_date == "2024-06-16"  # next_day applied for half-open interval

    def test_with_none_end_time_defaults_to_single_day(self) -> None:
        # Arrange
        start_time = "2024-03-15T08:00:00Z"

        # Act
        start_date, end_date = databento_definition_dates(start_time, None)

        # Assert
        assert start_date == "2024-03-15"
        assert end_date == "2024-03-16"  # next_day applied for half-open interval

    @pytest.mark.parametrize(
        ("start_time", "end_time", "expected_start", "expected_end"),
        [
            ("2024-01-01T00:00:00Z", "2024-01-01T00:00:00Z", "2024-01-01", "2024-01-02"),
            ("2024-06-15T12:00:00Z", "2024-06-20T18:00:00Z", "2024-06-15", "2024-06-21"),
            ("2023-02-28T00:00:00Z", "2023-02-28T23:59:59Z", "2023-02-28", "2023-03-01"),
            ("2024-02-28T00:00:00Z", "2024-02-29T23:59:59Z", "2024-02-28", "2024-03-01"),
        ],
    )
    def test_parametrized_date_ranges(
        self,
        start_time: str,
        end_time: str,
        expected_start: str,
        expected_end: str,
    ) -> None:
        # Act
        start_date, end_date = databento_definition_dates(start_time, end_time)

        # Assert
        assert start_date == expected_start
        assert end_date == expected_end
