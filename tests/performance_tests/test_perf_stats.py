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

import numpy as np

from nautilus_trader.core.stats import fast_mean
from nautilus_trader.core.stats import fast_std


def test_np_mean(benchmark):
    benchmark(
        np.mean,
        np.random.default_rng(10).random(100),
    )


def test_np_std(benchmark):
    benchmark(np.std, np.random.default_rng(10).random(100))


def test_fast_mean(benchmark):
    benchmark(fast_mean, np.random.default_rng(10).random(100))


def test_fast_std(benchmark):
    benchmark(fast_std, np.random.default_rng(10).random(100))
