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
Test enums behavior.
"""

import pytest

from nautilus_trader.model import AccountType
from nautilus_trader.model import AggressorSide
from nautilus_trader.model import BookType
from nautilus_trader.model import ContingencyType
from nautilus_trader.model import InstrumentClass
from nautilus_trader.model import MarketStatus
from nautilus_trader.model import OmsType
from nautilus_trader.model import OrderSide
from nautilus_trader.model import OrderType
from nautilus_trader.model import OtoTriggerMode
from nautilus_trader.model import PoolLiquidityUpdateType
from nautilus_trader.model import PositionSide
from nautilus_trader.model import TradingState
from nautilus_trader.model import TrailingOffsetType
from nautilus_trader.model import TriggerType


def test_model_enum_variants_are_iterable() -> None:
    """
    Test model enum variants are iterable.
    """
    variants = list(AccountType.variants())
    assert AccountType.CASH in variants
    assert AccountType.MARGIN in variants


@pytest.mark.parametrize(
    ("enum_type", "expected"),
    [
        (OrderSide, [OrderSide.BUY, OrderSide.SELL]),
        (PositionSide, [PositionSide.FLAT, PositionSide.LONG, PositionSide.SHORT]),
    ],
)
def test_side_enums_expose_only_domain_states(enum_type: object, expected: object) -> None:
    """
    Test side enums expose only valid domain states.
    """
    assert list(enum_type.variants()) == expected


def test_side_enums_retain_none_compatibility_aliases() -> None:
    """
    Test side enums retain transitional aliases for optional values.
    """
    assert OrderSide.NO_ORDER_SIDE is None
    assert PositionSide.NO_POSITION_SIDE is None


@pytest.mark.parametrize(
    ("enum_type", "token"),
    [
        (OrderSide, "NO_ORDER_SIDE"),
        (PositionSide, "NO_POSITION_SIDE"),
    ],
)
def test_side_enums_accept_legacy_none_tokens(enum_type: object, token: str) -> None:
    """
    Test side enums retain transitional parsing for optional values.
    """
    assert enum_type(token) is None
    assert enum_type.from_str(token) is None


@pytest.mark.parametrize(
    ("enum_type", "token", "expected"),
    [
        (OrderSide, "BUY", OrderSide.BUY),
        (OrderSide, "SELL", OrderSide.SELL),
        (PositionSide, "FLAT", PositionSide.FLAT),
        (PositionSide, "LONG", PositionSide.LONG),
        (PositionSide, "SHORT", PositionSide.SHORT),
    ],
)
def test_side_enums_accept_domain_tokens(
    enum_type: object,
    token: str,
    expected: object,
) -> None:
    """
    Test side enums retain normal parsing for valid domain states.
    """
    assert enum_type(token) == expected
    assert enum_type.from_str(token) == expected


@pytest.mark.parametrize(
    ("enum_type", "expected"),
    [
        (ContingencyType, [ContingencyType.OCO, ContingencyType.OTO, ContingencyType.OUO]),
        (
            TrailingOffsetType,
            [
                TrailingOffsetType.PRICE,
                TrailingOffsetType.BASIS_POINTS,
                TrailingOffsetType.TICKS,
                TrailingOffsetType.PRICE_TIER,
            ],
        ),
        (
            TriggerType,
            [
                TriggerType.DEFAULT,
                TriggerType.LAST_PRICE,
                TriggerType.MARK_PRICE,
                TriggerType.INDEX_PRICE,
                TriggerType.BID_ASK,
                TriggerType.DOUBLE_LAST,
                TriggerType.DOUBLE_BID_ASK,
                TriggerType.LAST_OR_BID_ASK,
                TriggerType.MID_POINT,
            ],
        ),
    ],
)
def test_optional_domain_enums_expose_only_domain_states(
    enum_type: object,
    expected: object,
) -> None:
    """
    Test optional domain enums expose only valid domain states.
    """
    assert list(enum_type.variants()) == expected


@pytest.mark.parametrize(
    ("enum_type", "alias", "token"),
    [
        (ContingencyType, "NO_CONTINGENCY", "NO_CONTINGENCY"),
        (TrailingOffsetType, "NO_TRAILING_OFFSET", "NO_TRAILING_OFFSET"),
        (TriggerType, "NO_TRIGGER", "NO_TRIGGER"),
    ],
)
def test_optional_domain_enums_retain_none_compatibility(
    enum_type: object,
    alias: str,
    token: str,
) -> None:
    """
    Test optional domain enums retain transitional aliases and parsing.
    """
    assert getattr(enum_type, alias) is None
    assert enum_type(token) is None
    assert enum_type.from_str(token) is None


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
def test_model_enums_from_str(enum_type: object, member: object, name: object) -> None:
    """
    Test model enums from str.
    """
    assert enum_type.from_str(name) == member
    assert member.name == name
    assert isinstance(hash(member), int)


def test_trading_state_values_progress_by_restriction() -> None:
    """
    Test trading state values progress by restriction.
    """
    assert TradingState.ACTIVE.value == 1
    assert TradingState.REDUCING.value == 2
    assert TradingState.HALTED.value == 3


def test_pool_liquidity_update_type_from_str() -> None:
    """
    Test pool liquidity update type from str.
    """
    assert PoolLiquidityUpdateType.from_str("Mint") == PoolLiquidityUpdateType.MINT


@pytest.mark.parametrize(
    ("member", "name"),
    [
        (AggressorSide.NO_AGGRESSOR, "NO_AGGRESSOR"),
        (AggressorSide.BUY, "BUY"),
        (AggressorSide.SELL, "SELL"),
    ],
)
def test_aggressor_side_canonical_names(member: object, name: object) -> None:
    """
    Test aggressor side canonical names.
    """
    assert member.name == name
    assert str(member) == name


@pytest.mark.parametrize(
    ("text", "member"),
    [
        ("BUY", AggressorSide.BUY),
        ("SELL", AggressorSide.SELL),
        ("BUYER", AggressorSide.BUY),
        ("SELLER", AggressorSide.SELL),
    ],
)
def test_aggressor_side_from_str_accepts_historical(text: object, member: object) -> None:
    """
    Test aggressor side from str accepts historical.
    """
    assert AggressorSide.from_str(text) == member


def test_aggressor_side_has_no_alias_members() -> None:
    """
    Test aggressor side has no alias members.
    """
    with pytest.raises(AttributeError):
        _ = AggressorSide.BUYER
    with pytest.raises(AttributeError):
        _ = AggressorSide.SELLER


@pytest.mark.parametrize("enum_type", [BookType, OrderSide, OrderType])
def test_workflow_enums_reject_malformed_values(enum_type: object) -> None:
    """
    Test workflow enums reject malformed values.
    """
    with pytest.raises(ValueError, match="Matching variant not found"):
        enum_type.from_str("NOT_A_VARIANT")
