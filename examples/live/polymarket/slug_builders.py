#!/usr/bin/env python3
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
"""
Example slug builder functions for Polymarket event_slug_builder feature.

These functions dynamically generate event slugs for niche markets with
predictable naming patterns, allowing efficient loading without downloading
all 151k+ markets.

Usage:
    In your PolymarketInstrumentProviderConfig:
    ```python
    instrument_config = PolymarketInstrumentProviderConfig(
        event_slug_builder="examples.live.polymarket.slug_builders:build_btc_updown_slugs",
    )
    ```

"""

from datetime import UTC
from datetime import datetime
from datetime import timedelta


def build_btc_updown_slugs() -> list[str]:
    """
    Build slugs for BTC 15-minute UpDown markets for the next few hours.

    UpDown markets follow the pattern: btc-updown-15m-{unix_timestamp}
    Where timestamp is the START of the 15-minute window.

    Returns
    -------
    list[str]
        List of event slugs for BTC UpDown markets.

    """
    slugs = []
    now = datetime.now(tz=UTC)

    # Round down to nearest 15-minute interval
    minutes = (now.minute // 15) * 15
    base_time = now.replace(minute=minutes, second=0, microsecond=0)

    # Generate slugs for the next 8 intervals (2 hours)
    for i in range(8):
        interval_time = base_time + timedelta(minutes=15 * i)
        timestamp = int(interval_time.timestamp())
        slug = f"btc-updown-15m-{timestamp}"
        slugs.append(slug)

    return slugs


def build_eth_updown_slugs() -> list[str]:
    """
    Build slugs for ETH 15-minute UpDown markets for the next few hours.

    Returns
    -------
    list[str]
        List of event slugs for ETH UpDown markets.

    """
    slugs = []
    now = datetime.now(tz=UTC)

    # Round down to nearest 15-minute interval
    minutes = (now.minute // 15) * 15
    base_time = now.replace(minute=minutes, second=0, microsecond=0)

    # Generate slugs for the next 8 intervals (2 hours)
    for i in range(8):
        interval_time = base_time + timedelta(minutes=15 * i)
        timestamp = int(interval_time.timestamp())
        slug = f"eth-updown-15m-{timestamp}"
        slugs.append(slug)

    return slugs


def build_crypto_updown_slugs() -> list[str]:
    """
    Build slugs for multiple crypto 15-minute UpDown markets (BTC, ETH, SOL).

    Returns
    -------
    list[str]
        List of event slugs for crypto UpDown markets.

    """
    cryptos = ["btc", "eth", "sol"]
    slugs = []
    now = datetime.now(tz=UTC)

    # Round down to nearest 15-minute interval
    minutes = (now.minute // 15) * 15
    base_time = now.replace(minute=minutes, second=0, microsecond=0)

    # Generate slugs for the next 4 intervals (1 hour) per crypto
    for i in range(4):
        interval_time = base_time + timedelta(minutes=15 * i)
        timestamp = int(interval_time.timestamp())
        for crypto in cryptos:
            slug = f"{crypto}-updown-15m-{timestamp}"
            slugs.append(slug)

    return slugs


def build_sample_slugs() -> list[str]:
    """
    Build a list of sample slugs for testing.

    Returns known active event slugs from Polymarket.

    Returns
    -------
    list[str]
        List of sample event slugs.

    """
    return [
        "presidential-election-winner-2024",
    ]
