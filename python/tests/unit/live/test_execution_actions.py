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
Test the public Python execution-action bindings.
"""

import pytest

from nautilus_trader.live import BinaryOptionAction
from nautilus_trader.model import InstrumentId
from nautilus_trader.model import Quantity


YES_ID = InstrumentId.from_str("42-YES-OUTCOME.HYPERLIQUID")
NO_ID = InstrumentId.from_str("42-NO-OUTCOME.HYPERLIQUID")


@pytest.mark.parametrize("operation", ["split", "merge"])
@pytest.mark.parametrize("params", [None, {"tag": "conversion"}])
def test_binary_option_action_preserves_public_fields(operation, params) -> None:
    """
    Preserve exact domain values and optional parameters through the Python wrapper.
    """
    quantity = Quantity.from_str("1.25")
    action = getattr(BinaryOptionAction, operation)(YES_ID, NO_ID, quantity, params)

    assert action.name == f"binary_option.{operation}"
    assert action.yes_instrument_id == YES_ID
    assert action.no_instrument_id == NO_ID
    assert action.quantity == quantity
    assert action.params == params


@pytest.mark.parametrize("operation", ["split", "merge"])
@pytest.mark.parametrize(
    ("no_id", "quantity", "message"),
    [
        (NO_ID, "0", "quantity must be positive"),
        (YES_ID, "1", "must differ"),
    ],
)
def test_binary_option_action_invalid_input_raises_value_error(
    operation,
    no_id,
    quantity,
    message,
) -> None:
    """
    Convert invalid action arguments into a Python ValueError.
    """
    with pytest.raises(ValueError, match=message):
        getattr(BinaryOptionAction, operation)(YES_ID, no_id, Quantity.from_str(quantity))
