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

from nautilus_trader.execution.matching_core import MatchingCore
from nautilus_trader.model.objects import Price
from nautilus_trader.test_kit.stubs.identifiers import TestIdStubs


def _noop(*args, **kwargs) -> None:
    return None


class TestMatchingCore:
    def setup(self) -> None:
        self.instrument_id = TestIdStubs.usdjpy_id()

    def test_update_price_increment_updates_internal_precision(self) -> None:
        initial_increment = Price.from_str("0.01")
        core = MatchingCore(
            instrument_id=self.instrument_id,
            price_increment=initial_increment,
            trigger_stop_order=_noop,
            fill_market_order=_noop,
            fill_limit_order=_noop,
        )

        assert core.price_increment == initial_increment
        assert core.price_increment.precision == 2
        assert core.price_precision == 2

        new_increment = Price.from_str("0.001")
        core.update_price_increment(new_increment)

        assert core.price_increment == new_increment
        assert core.price_increment.precision == 3
        assert core.price_precision == 3
