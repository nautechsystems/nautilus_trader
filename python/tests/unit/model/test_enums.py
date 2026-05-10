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

from nautilus_trader.model import AccountType
from nautilus_trader.model import InstrumentClass
from nautilus_trader.model import MarketStatus
from nautilus_trader.model import OmsType
from nautilus_trader.model import OtoTriggerMode
from nautilus_trader.model import PoolLiquidityUpdateType
from nautilus_trader.model import TradingState


def test_model_enum_variants_are_iterable():
    variants = list(AccountType.variants())
    assert AccountType.CASH in variants
    assert AccountType.MARGIN in variants


@pytest.mark.parametrize(
    ("enum_type", "member", "name"),
    [
        (InstrumentClass, InstrumentClass.SPOT, "SPOT"),
        (MarketStatus, MarketStatus.OPEN, "OPEN"),
        (OmsType, OmsType.NETTING, "NETTING"),
        (OtoTriggerMode, OtoTriggerMode.FULL, "FULL"),
        (TradingState, TradingState.ACTIVE, "ACTIVE"),
    ],
)
def test_model_enums_from_str(enum_type, member, name):
    assert enum_type.from_str(name) == member
    assert member.name == name
    assert isinstance(hash(member), int)


def test_pool_liquidity_update_type_from_str():
    assert PoolLiquidityUpdateType.from_str("Mint") == PoolLiquidityUpdateType.MINT
