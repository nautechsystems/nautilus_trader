# -------------------------------------------------------------------------------------------------
#  Copyright (C) 2015-2023 Nautech Systems Pty Ltd. All rights reserved.
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

from nautilus_trader.adapters.databento.loaders import DatabentoDataLoader
from nautilus_trader.model.instruments import Equity
from tests import TEST_DATA_DIR


def test_loader_with_futures_contract() -> None:
    # Arrange
    loader = DatabentoDataLoader()
    path = TEST_DATA_DIR / "databento/definition.dbn.zst"

    data = loader.from_dbn(path)

    # Assert
    assert len(data) == 2
    assert isinstance(data[0], Equity)
    assert (
        repr(data[0])
        == "Equity(id=MSFT.XNAS, raw_symbol=MSFT, asset_class=EQUITY, asset_type=SPOT, quote_currency=USD, is_inverse=False, price_precision=2, price_increment=0.01, size_precision=0, size_increment=1, multiplier=1, lot_size=100, margin_init=0, margin_maint=0, maker_fee=0, taker_fee=0, info=None)"  # noqa
    )
