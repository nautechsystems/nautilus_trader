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

from nautilus_trader.backtest.models import FillModel


_FILL_MODEL = FillModel(
    prob_fill_on_stop=0.95,
    prob_fill_on_limit=0.5,
    random_seed=42,
)


def test_is_limit_filled(benchmark):
    benchmark(_FILL_MODEL.is_limit_filled)


def test_is_stop_filled(benchmark):
    benchmark(_FILL_MODEL.is_stop_filled)
