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

from nautilus_trader.core.nautilus_pyo3 import InstrumentId
from nautilus_trader.core.nautilus_pyo3 import InstrumentStatus
from nautilus_trader.core.nautilus_pyo3 import MarketStatusAction


def test_instrument_status():
    # Arrange
    update = InstrumentStatus(
        instrument_id=InstrumentId.from_str("MSFT.XNAS"),
        action=MarketStatusAction.TRADING,
        ts_event=0,
        ts_init=0,
        reason=None,
        trading_event=None,
        is_trading=True,
        is_quoting=True,
        is_short_sell_restricted=False,
    )

    # Act, Assert
    assert InstrumentStatus.from_dict(InstrumentStatus.as_dict(update)) == update
    assert repr(update) == "InstrumentStatus(MSFT.XNAS,TRADING,0,0)"  # TODO: Improve repr from Rust
