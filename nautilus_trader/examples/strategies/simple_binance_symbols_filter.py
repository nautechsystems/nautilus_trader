# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2024 Nautech Systems Pty Ltd. All rights reserved.
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
from datetime import datetime, timedelta
from typing import Optional, Tuple

import pandas as pd
import requests


def extract_symbol_info(
    api_url: str = "https://fapi.binance.com/fapi/v1/exchangeInfo"
) -> tuple[pd.DataFrame | None, str | None]:
    """Fetch and extract symbol info from Binance Futures API and return it as a DataFrame.
    
    Args:
    - api_url (str): The API URL to fetch data from (default: Binance Futures API).

    Returns:
    - Tuple containing the DataFrame and an optional error message.
    """
    try:
        # Send a GET request to the Binance API
        response = requests.get(api_url)
        response.raise_for_status()

        # Load the JSON response
        json_data = response.json()

        # Extract symbols data from JSON
        symbols = json_data['symbols']
        df = pd.DataFrame(symbols)

        # Filter DataFrame for 'PERPETUAL' contract type and 'TRADING' status
        df = df[(df['contractType'] == 'PERPETUAL') & (df['status'] == 'TRADING')]

        return df, None

    except requests.exceptions.RequestException as e:
        return None, f"Request error: {e}"
    except json.JSONDecodeError:
        return None, "Error decoding JSON"
    except KeyError as e:
        return None, f"Unexpected JSON structure, missing key: {e}"


def select_with_quoteAsset(df: pd.DataFrame, quoteAsset: str) -> pd.DataFrame:
    """Filter DataFrame for products with a specific quote asset.

    Args:
    - df (pd.DataFrame): The DataFrame to filter.
    - quoteAsset (str): The quote asset to filter by.

    Returns:
    - pd.DataFrame: Filtered DataFrame.
    """
    return df[df['quoteAsset'] == quoteAsset]

def select_with_min_notional(df: pd.DataFrame, min_notional_threshold: float) -> pd.DataFrame:
    """Add a min_notional column to the DataFrame and filter it based on a threshold.

    Args:
    - df (pd.DataFrame): The DataFrame to process.
    - min_notional_threshold (float): The threshold to filter min_notional values.

    Returns:
    - pd.DataFrame: DataFrame with min_notional columns and filtered values.
    """
    # Extract 'MIN_NOTIONAL' from filters and set it in the DataFrame
    df = df.copy()
    df['min_notional'] = df['filters'].apply(lambda x: next(
        (float(filter_item.get('notional', 0)) for filter_item in x if filter_item.get('filterType') == 'MIN_NOTIONAL'), 0))
    # Filter the DataFrame where min_notional is less than the threshold
    return df[df['min_notional'] < min_notional_threshold]


def filter_with_onboard_date(
    df: pd.DataFrame,
    filter_type: str,
    reference_date: datetime,
    end_date: datetime | None = None,
) -> pd.DataFrame:
    """Filter the DataFrame based on onboardDate according to the specified criteria.

    Args:
    - df (pd.DataFrame): The DataFrame containing the onboardDate column.
    - filter_type (str): The type of filter to apply. Can be 'before', 'range', or 'after'.
    - reference_date (datetime): The date to use for filtering.
    - end_date (datetime, optional): The end date for 'range' filter type. Required if filter_type is 'range'.

    Returns:
    - pd.DataFrame: Filtered DataFrame based on the specified filter type and dates.
    """
    # Convert onboardDate from milliseconds timestamp to datetime
    df = df.copy()
    df['onboardDate_convert'] = pd.to_datetime(df['onboardDate'], unit='ms')

    if filter_type == 'before':
        # Filter the DataFrame for onboardDate earlier than the reference date
        filtered_df = df[df['onboardDate_convert'] < reference_date]
    elif filter_type == 'range':
        if end_date is None:
            raise ValueError("end_date must be provided for 'range' filter type.")
        # Filter the DataFrame for onboardDate within the specified date range
        filtered_df = df[(df['onboardDate_convert'] >= reference_date) & (df['onboardDate_convert'] <= end_date)]
    elif filter_type == 'after':
        # Filter the DataFrame for onboardDate later than the reference date
        filtered_df = df[df['onboardDate_convert'] > reference_date]
    else:
        raise ValueError("filter_type must be one of 'before', 'range', or 'after'.")

    return filtered_df
