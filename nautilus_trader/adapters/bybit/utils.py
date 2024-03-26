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
import os.path
import time
from decimal import Decimal
from typing import Any

import msgspec

from nautilus_trader.adapters.bybit.common.constants import BYBIT_VALID_HOUR_INTERVALS
from nautilus_trader.adapters.bybit.common.constants import BYBIT_VALID_MINUTE_INTERVALS
from nautilus_trader.adapters.bybit.common.enums import BybitInstrumentType
from nautilus_trader.adapters.env import get_env_key
from nautilus_trader.model.data import BarType
from nautilus_trader.model.enums import BarAggregation
from nautilus_trader.model.enums import bar_aggregation_to_str


def msgspec_bybit_item_save(filename: str, obj: Any) -> None:
    item = msgspec.to_builtins(obj)
    timestamp = round(time.time() * 1000)
    item_json = json.dumps(
        {"retCode": 0, "retMsg": "success", "time": timestamp, "result": item},
        indent=4,
    )
    # check if the file already exists, if exists, do not overwrite
    if os.path.isfile(filename):
        return
    with open(filename, "w", encoding="utf-8") as f:
        f.write(item_json)


def get_category_from_instrument_type(instrument_type: BybitInstrumentType) -> str:
    if instrument_type == BybitInstrumentType.SPOT:
        return "spot"
    elif instrument_type == BybitInstrumentType.LINEAR:
        return "linear"
    elif instrument_type == BybitInstrumentType.INVERSE:
        return "inverse"
    elif instrument_type == BybitInstrumentType.OPTION:
        return "option"
    else:
        raise ValueError(f"Unknown account type: {instrument_type}")


def get_interval_from_bar_type(bar_type: BarType) -> str:
    aggregation: BarAggregation = bar_type.spec.aggregation
    match aggregation:
        case BarAggregation.MINUTE:
            if bar_type.spec.step not in BYBIT_VALID_MINUTE_INTERVALS:
                raise ValueError(
                    f"Bybit only supports the following bar minute intervals: "
                    f"{BYBIT_VALID_MINUTE_INTERVALS}",
                )
            return str(bar_type.spec.step)
        case BarAggregation.HOUR:
            if bar_type.spec.step not in BYBIT_VALID_HOUR_INTERVALS:
                raise ValueError(
                    f"Bybit only supports the following bar hour intervals: "
                    f"{BYBIT_VALID_HOUR_INTERVALS}",
                )
            return str(bar_type.spec.step * 60)
        case BarAggregation.DAY:
            if bar_type.spec.step != 1:
                raise ValueError("Bybit only supports 1 DAY interval bars")
            return "D"
        case BarAggregation.WEEK:
            if bar_type.spec.step == 1:
                return "W"
            if bar_type.spec.step == 4:
                return "M"
            raise ValueError("Bybit only supports 1 WEEK interval bars")
        case _:
            raise ValueError(
                f"Bybit does not support {bar_aggregation_to_str(bar_type.aggregation)} bars",
            )


def tick_size_to_precision(tick_size: float | Decimal) -> int:
    tick_size_str = f"{tick_size:.10f}"
    return len(tick_size_str.partition(".")[2].rstrip("0"))


def get_api_key(is_testnet: bool) -> str:
    if is_testnet:
        return get_env_key("BYBIT_TESTNET_API_KEY")
    else:
        return get_env_key("BYBIT_API_KEY")


def get_api_secret(is_testnet: bool) -> str:
    if is_testnet:
        return get_env_key("BYBIT_TESTNET_API_SECRET")
    else:
        return get_env_key("BYBIT_API_SECRET")
