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
Test the Interactive Brokers instrument provider contract details lookup.
"""

import asyncio
import json
from datetime import UTC
from datetime import datetime
from pathlib import Path

from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProvider
from nautilus_trader.adapters.interactive_brokers import InteractiveBrokersInstrumentProviderConfig
from nautilus_trader.model import InstrumentId


SPY_ID = InstrumentId.from_str("SPY=STK.ARCA")


SPY_CACHE_ENTRY: tuple[str, dict] = (
    "SPY=STK.ARCA",
    {
        "agg_group": 0,
        "bond_type": "",
        "callable": False,
        "category": "",
        "contract": {
            "combo_legs": [],
            "combo_legs_description": "",
            "contract_id": 0,
            "currency": "USD",
            "delta_neutral_contract": None,
            "description": "",
            "exchange": "SMART",
            "include_expired": False,
            "issuer_id": "",
            "last_trade_date": None,
            "last_trade_date_or_contract_month": "",
            "local_symbol": "",
            "multiplier": "",
            "primary_exchange": "",
            "right": None,
            "security_id": "",
            "security_id_type": None,
            "security_type": "Stock",
            "strike": 0.0,
            "symbol": "",
            "trading_class": "",
        },
        "contract_month": "",
        "convertible": False,
        "coupon": 0.0,
        "coupon_type": "",
        "cusip": "",
        "desc_append": "",
        "ev_multiplier": 0.0,
        "ev_rule": "",
        "fund_asset_type": "None",
        "fund_back_load": "",
        "fund_back_load_time_interval": "",
        "fund_blue_sky_states": "",
        "fund_blue_sky_territories": "",
        "fund_closed": False,
        "fund_closed_for_new_investors": False,
        "fund_closed_for_new_money": False,
        "fund_distribution_policy_indicator": "None",
        "fund_family": "",
        "fund_front_load": "",
        "fund_management_fee": "",
        "fund_minimum_initial_purchase": "",
        "fund_name": "",
        "fund_notify_amount": "",
        "fund_subsequent_minimum_purchase": "",
        "fund_type": "",
        "industry": "",
        "ineligibility_reasons": [],
        "issue_date": "",
        "last_trade_time": "",
        "liquid_hours": ["20260922:0930-20260922:1600"],
        "long_name": "",
        "market_name": "",
        "market_rule_ids": [],
        "maturity": "",
        "min_size": 0.0,
        "min_tick": 0.0,
        "next_option_date": "",
        "next_option_partial": False,
        "next_option_type": "",
        "notes": "",
        "order_types": [],
        "price_magnifier": 0,
        "putable": False,
        "ratings": "",
        "real_expiration_date": "",
        "sec_id_list": [],
        "size_increment": 0.0,
        "stock_type": "",
        "subcategory": "",
        "suggested_size_increment": 0.0,
        "time_zone_id": "America/New_York",
        "trading_hours": ["20260922:0930-20260922:1600", "20260923:0930-20260923:1600"],
        "under_contract_id": 0,
        "under_security_type": "",
        "under_symbol": "",
        "valid_exchanges": [],
    },
)


def _load_contract_details_cache(
    provider: InteractiveBrokersInstrumentProvider,
    tmp_path: Path,
) -> None:
    cache = {
        "cache_timestamp": datetime.now(UTC).isoformat().replace("+00:00", "Z"),
        "contract_id_to_instrument_id": [],
        "price_magnifiers": [],
        "contracts": [],
        "contract_details": [SPY_CACHE_ENTRY],
        "instruments": [],
    }
    path = tmp_path / "_contract_cache_test.json"
    path.write_text(json.dumps(cache), encoding="utf-8")

    async def load() -> bool:
        return await provider.load_cache(str(path))

    assert asyncio.run(load()) is True


def test_instrument_id_to_ib_contract_details_exposes_trading_hours(tmp_path: Path) -> None:
    """
    Test cached contract details convert without importing a missing Python module.
    """
    provider = InteractiveBrokersInstrumentProvider(InteractiveBrokersInstrumentProviderConfig())
    _load_contract_details_cache(provider, tmp_path)

    details = provider.instrument_id_to_ib_contract_details(SPY_ID)

    assert details is not None
    assert details["timeZoneId"] == "America/New_York"
    assert details["tradingHours"] == "20260922:0930-20260922:1600;20260923:0930-20260923:1600"
